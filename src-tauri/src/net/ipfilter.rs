//! Política de direcciones IP: a quién se le permite conectarse a DameBola.
//!
//! Una IP «privada» no basta para confiar: el descubrimiento (mDNS) es multicast no autenticado, así
//! que un candidato debe además estar **en el mismo enlace** (subred de una de nuestras interfaces).
//! La identidad real siempre la decide TLS con pinning; esto solo limita *a dónde* se intenta conectar.
//!
//! | Origen | Clases admitidas | ¿Debe estar en nuestra subred? |
//! |---|---|---|
//! | `Discovery` (mDNS) | privada, link-local | **sí** |
//! | `Stored` (dirección ya guardada de un emparejado) | privada, link-local | no (pudo agregarse a mano en otra VLAN) |
//! | `Manual` (el usuario la escribió) | privada, link-local | no |
//!
//! Loopback solo si `allow_loopback` (pruebas / desarrollo aislado). Nunca: públicas, CGNAT
//! (100.64.0.0/10), no especificada, multicast, broadcast, documentación, benchmarking, reservadas.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpClass {
    Private,
    LinkLocal,
    Loopback,
    /// 100.64.0.0/10 (compartido por operadores/VPN tipo Tailscale): no es una LAN.
    CgNat,
    Unspecified,
    Multicast,
    Broadcast,
    Documentation,
    Benchmark,
    Reserved,
    /// IPv6 link-local: requiere identificador de interfaz, no soportado.
    LinkLocalV6,
    Public,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Discovery,
    Stored,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    Class(IpClass),
    NotOnLink,
    OwnAddress,
}

impl Reject {
    pub fn message(&self) -> String {
        match self {
            Self::Class(IpClass::Public) => "Esa dirección es pública (de Internet). DameBola solo se conecta a equipos de la red local.".into(),
            Self::Class(IpClass::Loopback) => "Esa es la dirección de este mismo equipo.".into(),
            Self::Class(IpClass::CgNat) => "Esa dirección es de una red compartida (CGNAT/VPN), no de tu red local.".into(),
            Self::Class(IpClass::Unspecified | IpClass::Multicast | IpClass::Broadcast | IpClass::Documentation | IpClass::Benchmark | IpClass::Reserved) => {
                "Esa dirección no es válida para conectarse a un equipo.".into()
            }
            Self::Class(IpClass::LinkLocalV6) => "Las direcciones IPv6 link-local no están soportadas.".into(),
            Self::Class(_) => "Dirección no permitida.".into(),
            Self::NotOnLink => "Esa dirección no está en una de tus redes locales.".into(),
            Self::OwnAddress => "Esa es una dirección de este mismo equipo.".into(),
        }
    }
}

/// Una interfaz IPv4 local con su máscara.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalNet {
    pub ip: Ipv4Addr,
    pub mask: Ipv4Addr,
}

impl LocalNet {
    fn network(&self) -> u32 {
        u32::from(self.ip) & u32::from(self.mask)
    }

    fn broadcast(&self) -> u32 {
        self.network() | !u32::from(self.mask)
    }

    /// ¿`ip` es un *host* de esta subred (ni red, ni broadcast, ni nosotros)?
    fn contains_host(&self, ip: Ipv4Addr) -> bool {
        let v = u32::from(ip);
        // Máscaras /31 y /32 no tienen noción de «dirección de red/broadcast» útil: no se aceptan.
        self.mask != Ipv4Addr::BROADCAST && (u32::from(self.mask) & 0xFFFF_FFFE) != 0xFFFF_FFFE && v & u32::from(self.mask) == self.network() && v != self.network() && v != self.broadcast()
    }
}

pub fn classify_v4(ip: Ipv4Addr) -> IpClass {
    let o = ip.octets();
    match o {
        [0, ..] => IpClass::Unspecified,
        [127, ..] => IpClass::Loopback,
        [10, ..] => IpClass::Private,
        [172, b, ..] if (16..=31).contains(&b) => IpClass::Private,
        [192, 168, ..] => IpClass::Private,
        [169, 254, ..] => IpClass::LinkLocal,
        [100, b, ..] if (64..=127).contains(&b) => IpClass::CgNat,
        [255, 255, 255, 255] => IpClass::Broadcast,
        [224..=239, ..] => IpClass::Multicast,
        [192, 0, 2, _] | [198, 51, 100, _] | [203, 0, 113, _] => IpClass::Documentation,
        [198, b, ..] if b == 18 || b == 19 => IpClass::Benchmark,
        [192, 0, 0, _] | [192, 88, 99, _] => IpClass::Reserved,
        [240..=255, ..] => IpClass::Reserved,
        _ => IpClass::Public,
    }
}

pub fn classify(ip: IpAddr) -> IpClass {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => classify_v6(v6),
    }
}

fn classify_v6(ip: Ipv6Addr) -> IpClass {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return classify_v4(v4);
    }
    let seg0 = ip.segments()[0];
    if ip.is_unspecified() {
        IpClass::Unspecified
    } else if ip.is_loopback() {
        IpClass::Loopback
    } else if ip.is_multicast() {
        IpClass::Multicast
    } else if (seg0 & 0xfe00) == 0xfc00 {
        IpClass::Private // ULA fc00::/7
    } else if (seg0 & 0xffc0) == 0xfe80 {
        IpClass::LinkLocalV6
    } else if seg0 == 0x2001 && ip.segments()[1] == 0x0db8 {
        IpClass::Documentation
    } else {
        IpClass::Public
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AddrPolicy {
    pub allow_loopback: bool,
}

impl AddrPolicy {
    pub fn check(&self, ip: IpAddr, origin: Origin, local: &[LocalNet]) -> Result<(), Reject> {
        let class = classify(ip);
        let class_ok = match class {
            IpClass::Private | IpClass::LinkLocal => true,
            IpClass::Loopback => self.allow_loopback,
            _ => false,
        };
        if !class_ok {
            return Err(Reject::Class(class));
        }
        if class == IpClass::Loopback {
            return Ok(()); // solo llega aquí con `allow_loopback` (pruebas / desarrollo aislado)
        }
        let IpAddr::V4(v4) = normalize(ip) else {
            // IPv6 (ULA): solo origen explícito/almacenado; el descubrimiento es IPv4.
            return if origin == Origin::Discovery { Err(Reject::NotOnLink) } else { Ok(()) };
        };
        if local.iter().any(|n| n.ip == v4) {
            return Err(Reject::OwnAddress);
        }
        if origin == Origin::Discovery && !local.iter().any(|n| n.contains_host(v4) && n.ip != v4) {
            return Err(Reject::NotOnLink);
        }
        Ok(())
    }
}

/// `::ffff:a.b.c.d` → IPv4.
pub fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        v4 => v4,
    }
}

/// Interfaces IPv4 activas con su máscara (todas: con varias interfaces, un candidato es válido si
/// está en el enlace de *cualquiera* de ellas).
pub fn local_nets() -> Vec<LocalNet> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| i.is_oper_up())
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(v4) => Some(LocalNet { ip: v4.ip, mask: v4.netmask }),
            _ => None,
        })
        .collect()
}

/// Direcciones locales en las que se escucha: IPv4 privadas o link-local de interfaces activas.
pub fn listen_addresses() -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = local_nets()
        .into_iter()
        .filter(|n| matches!(classify_v4(n.ip), IpClass::Private | IpClass::LinkLocal))
        .map(|n| IpAddr::V4(n.ip))
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(s: &str) -> IpAddr {
        s.parse().unwrap()
    }
    fn net(ip: &str, mask: &str) -> LocalNet {
        LocalNet { ip: ip.parse().unwrap(), mask: mask.parse().unwrap() }
    }
    const P: AddrPolicy = AddrPolicy { allow_loopback: false };
    const L: AddrPolicy = AddrPolicy { allow_loopback: true };

    #[test]
    fn classifies_ipv4_ranges() {
        use IpClass::*;
        for (ip, class) in [
            ("10.0.0.1", Private), ("10.255.255.254", Private), ("172.16.0.1", Private), ("172.31.255.1", Private), ("192.168.0.1", Private),
            ("172.15.255.255", Public), ("172.32.0.1", Public), ("192.167.1.1", Public), ("11.0.0.1", Public),
            ("169.254.10.20", LinkLocal), ("169.253.0.1", Public),
            ("127.0.0.1", Loopback), ("127.255.255.254", Loopback),
            ("0.0.0.0", Unspecified), ("0.1.2.3", Unspecified),
            ("100.64.0.1", CgNat), ("100.127.255.254", CgNat), ("100.63.255.255", Public), ("100.128.0.1", Public),
            ("224.0.0.251", Multicast), ("239.255.255.250", Multicast),
            ("255.255.255.255", Broadcast), ("240.0.0.1", Reserved), ("254.1.1.1", Reserved),
            ("192.0.2.5", Documentation), ("198.51.100.7", Documentation), ("203.0.113.9", Documentation),
            ("198.18.0.1", Benchmark), ("198.19.255.1", Benchmark), ("192.0.0.8", Reserved), ("192.88.99.1", Reserved),
            ("8.8.8.8", Public), ("1.1.1.1", Public), ("93.184.216.34", Public),
        ] {
            assert_eq!(classify(v4(ip)), class, "{ip}");
        }
    }

    #[test]
    fn classifies_ipv6_and_mapped_addresses() {
        use IpClass::*;
        assert_eq!(classify(v4("::1")), Loopback);
        assert_eq!(classify(v4("::")), Unspecified);
        assert_eq!(classify(v4("fd12:3456::1")), Private);
        assert_eq!(classify(v4("fe80::1")), LinkLocalV6);
        assert_eq!(classify(v4("ff02::fb")), Multicast);
        assert_eq!(classify(v4("2001:db8::1")), Documentation);
        assert_eq!(classify(v4("2606:4700::1111")), Public);
        assert_eq!(classify(v4("::ffff:192.168.1.5")), Private, "IPv4-mapped se trata como IPv4");
        assert_eq!(classify(v4("::ffff:8.8.8.8")), Public, "no se esquiva el filtro con la forma mapeada");
    }

    #[test]
    fn public_and_special_addresses_are_rejected_for_every_origin() {
        let nets = [net("192.168.1.10", "255.255.255.0")];
        for origin in [Origin::Discovery, Origin::Stored, Origin::Manual] {
            for ip in ["8.8.8.8", "100.64.1.1", "0.0.0.0", "224.0.0.251", "255.255.255.255", "192.0.2.1", "198.18.0.1", "240.0.0.1", "169.253.1.1", "::ffff:8.8.8.8", "2606:4700::1111", "fe80::1", "::"] {
                assert!(P.check(v4(ip), origin, &nets).is_err(), "{ip} {origin:?}");
            }
        }
    }

    #[test]
    fn loopback_only_when_explicitly_allowed() {
        let nets = [net("127.0.0.1", "255.0.0.0")];
        assert_eq!(P.check(v4("127.0.0.1"), Origin::Manual, &nets), Err(Reject::Class(IpClass::Loopback)));
        assert_eq!(P.check(v4("::1"), Origin::Manual, &nets), Err(Reject::Class(IpClass::Loopback)));
        assert!(L.check(v4("127.0.0.1"), Origin::Manual, &nets).is_ok());
        assert!(L.check(v4("127.0.0.1"), Origin::Stored, &nets).is_ok());
    }

    #[test]
    fn discovery_requires_being_on_the_same_link_but_manual_and_stored_do_not() {
        let nets = [net("192.168.1.10", "255.255.255.0")];
        assert!(P.check(v4("192.168.1.77"), Origin::Discovery, &nets).is_ok());
        // Privada pero de OTRA subred (p. ej. rango corporativo enrutado): no se acepta por mDNS...
        assert_eq!(P.check(v4("10.20.30.40"), Origin::Discovery, &nets), Err(Reject::NotOnLink));
        assert_eq!(P.check(v4("192.168.2.5"), Origin::Discovery, &nets), Err(Reject::NotOnLink));
        // ...pero sí si el usuario la escribió o ya estaba guardada de un emparejado.
        assert!(P.check(v4("10.20.30.40"), Origin::Manual, &nets).is_ok());
        assert!(P.check(v4("10.20.30.40"), Origin::Stored, &nets).is_ok());
    }

    #[test]
    fn network_broadcast_and_own_addresses_are_not_valid_peers() {
        let nets = [net("192.168.1.10", "255.255.255.0")];
        assert_eq!(P.check(v4("192.168.1.0"), Origin::Discovery, &nets), Err(Reject::NotOnLink));
        assert_eq!(P.check(v4("192.168.1.255"), Origin::Discovery, &nets), Err(Reject::NotOnLink));
        assert_eq!(P.check(v4("192.168.1.10"), Origin::Discovery, &nets), Err(Reject::OwnAddress));
        assert_eq!(P.check(v4("192.168.1.10"), Origin::Manual, &nets), Err(Reject::OwnAddress));
    }

    #[test]
    fn multiple_interfaces_accept_a_peer_on_any_attached_link() {
        let nets = [net("192.168.1.10", "255.255.255.0"), net("10.8.0.5", "255.255.0.0"), net("169.254.7.7", "255.255.0.0")];
        assert!(P.check(v4("192.168.1.20"), Origin::Discovery, &nets).is_ok());
        assert!(P.check(v4("10.8.44.1"), Origin::Discovery, &nets).is_ok());
        assert!(P.check(v4("169.254.1.2"), Origin::Discovery, &nets).is_ok());
        assert!(P.check(v4("10.9.0.1"), Origin::Discovery, &nets).is_err());
        // Sin interfaces conocidas no se puede validar el enlace: el descubrimiento se rechaza.
        assert_eq!(P.check(v4("192.168.1.20"), Origin::Discovery, &[]), Err(Reject::NotOnLink));
        assert!(P.check(v4("192.168.1.20"), Origin::Manual, &[]).is_ok());
    }

    #[test]
    fn tiny_subnets_have_no_usable_peer_hosts() {
        let nets = [net("192.168.5.1", "255.255.255.255"), net("192.168.6.1", "255.255.255.254")];
        assert!(P.check(v4("192.168.5.2"), Origin::Discovery, &nets).is_err());
        assert!(P.check(v4("192.168.6.0"), Origin::Discovery, &nets).is_err());
    }

    #[test]
    fn ipv6_ula_only_for_explicit_origins() {
        assert!(P.check(v4("fd00::5"), Origin::Manual, &[]).is_ok());
        assert!(P.check(v4("fd00::5"), Origin::Stored, &[]).is_ok());
        assert!(P.check(v4("fd00::5"), Origin::Discovery, &[]).is_err());
    }

    #[test]
    fn rejection_messages_are_in_spanish() {
        assert!(Reject::Class(IpClass::Public).message().contains("pública"));
        assert!(Reject::NotOnLink.message().contains("red"));
    }
}
