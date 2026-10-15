//! Validación y normalización del atajo de teclado (ventana enfocada).
//!
//! Formato canónico: `F8`, `Ctrl+Shift+B`. Orden de modificadores: Ctrl, Alt, Shift, Meta.
//! Las mismas reglas están en `src/lib/shortcut.ts`; el backend es la autoridad.

pub const DEFAULT_SHORTCUT: &str = "F12";

/// Ctrl/Cmd + estas letras ya significan otra cosa (editar, cerrar, recargar).
const RESERVED_KEYS: &str = "ACVXZYWQRNT";

#[derive(Default, Clone, Copy)]
struct Mods {
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
}

/// Devuelve la forma canónica o un mensaje en español.
pub fn normalize(raw: &str) -> Result<String, String> {
    let tokens: Vec<&str> = raw.split('+').map(str::trim).collect();
    if tokens.iter().any(|t| t.is_empty()) {
        return Err("Formato de atajo inválido.".into());
    }
    let (key_token, mod_tokens) = tokens.split_last().ok_or("Formato de atajo inválido.")?;

    let mut mods = Mods::default();
    for token in mod_tokens {
        let slot = match token.to_lowercase().as_str() {
            "ctrl" | "control" => &mut mods.ctrl,
            "alt" | "option" => &mut mods.alt,
            "shift" => &mut mods.shift,
            "meta" | "cmd" | "command" | "win" | "super" => &mut mods.meta,
            _ => return Err(format!("«{token}» no es un modificador válido.")),
        };
        if *slot {
            return Err("Un modificador está repetido.".into());
        }
        *slot = true;
    }

    let function = parse_function_key(key_token);
    let key = match function {
        Some(n) => format!("F{n}"),
        None => {
            let mut chars = key_token.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => c.to_ascii_uppercase().to_string(),
                _ => return Err("Usá una tecla de función (F1–F24), una letra o un número.".into()),
            }
        }
    };

    if function.is_some() {
        if function == Some(4) && mods.alt {
            return Err("Alt + F4 cierra la ventana.".into());
        }
    } else {
        if !mods.ctrl && !mods.meta {
            return Err("Las letras y números necesitan Ctrl (o Cmd) para no enviar alertas al escribir.".into());
        }
        if mods.ctrl && mods.alt {
            return Err("Ctrl + Alt equivale a AltGr en muchos teclados y se usa para escribir.".into());
        }
        if RESERVED_KEYS.contains(key.as_str()) && !mods.alt {
            return Err(format!("Ctrl/Cmd + {key} ya tiene otro uso (copiar, pegar, cerrar, recargar…). Elegí otra combinación."));
        }
    }

    let mut parts: Vec<&str> = Vec::new();
    if mods.ctrl {
        parts.push("Ctrl");
    }
    if mods.alt {
        parts.push("Alt");
    }
    if mods.shift {
        parts.push("Shift");
    }
    if mods.meta {
        parts.push("Meta");
    }
    parts.push(&key);
    Ok(parts.join("+"))
}

/// Reglas adicionales para un atajo GLOBAL (el sistema operativo entrega la tecla aunque otra app tenga el foco).
/// `canonical` ya pasó por `normalize`. `os` es `net::service::current_os()` ("macos", "windows", …).
/// Rechaza combinaciones que el sistema reserva; las que ocupen otras apps las detecta el registro.
pub fn validate_global(canonical: &str, os: &str) -> Result<(), String> {
    let parts: Vec<&str> = canonical.split('+').collect();
    let (key, mods) = parts.split_last().ok_or("Formato de atajo inválido.")?;
    let has = |m: &str| mods.contains(&m);
    match os {
        "windows" => {
            if has("Meta") {
                return Err("Windows reserva las combinaciones con la tecla Windows (Win + …). Usá Ctrl, Alt o Shift.".into());
            }
            if has("Ctrl") && has("Shift") && *key == "F10" {
                return Err("Ctrl + Shift + F10 abre menús contextuales del sistema.".into());
            }
        }
        "macos" => {
            if has("Meta") {
                let reserved: &[(&[&str], &str)] = &[
                    (&[], "Q"), (&[], "H"), (&[], "M"), (&[], "W"), (&[], "F"),
                    (&["Shift"], "Q"), (&["Shift"], "3"), (&["Shift"], "4"), (&["Shift"], "5"), (&["Shift"], "6"),
                    (&["Ctrl"], "Q"), (&["Alt"], "H"), (&["Alt"], "M"), (&["Alt"], "D"),
                ];
                let mut sorted: Vec<&str> = mods.iter().copied().filter(|m| *m != "Meta").collect();
                sorted.sort_unstable();
                if reserved.iter().any(|(extra, k)| *k == *key && extra.iter().copied().eq(sorted.iter().copied())) {
                    return Err(format!("macOS reserva Cmd + {} para el sistema. Elegí otra combinación.", describe_mods(mods, key)));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn describe_mods(mods: &[&str], key: &str) -> String {
    let rest: Vec<&str> = mods.iter().copied().filter(|m| *m != "Meta").collect();
    if rest.is_empty() { key.to_string() } else { format!("{} + {key}", rest.join(" + ")) }
}

/// Texto que entiende `tauri-plugin-global-shortcut` ("Ctrl+Shift+B", "Super+K", "F8").
pub fn to_plugin_string(canonical: &str) -> String {
    canonical.split('+').map(|p| if p == "Meta" { "Super" } else { p }).collect::<Vec<_>>().join("+")
}

/// `F1`..`F24` (sin ceros a la izquierda).
fn parse_function_key(token: &str) -> Option<u8> {
    let rest = token.strip_prefix(['F', 'f'])?;
    if rest.is_empty() || rest.starts_with('0') || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse::<u8>().ok().filter(|n| (1..=24).contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mismos vectores que src/lib/shortcut.test.ts.
    #[test]
    fn accepts_and_normalizes() {
        for (input, expected) in [
            ("F12", "F12"), ("f8", "F8"), ("F24", "F24"), ("Shift+F5", "Shift+F5"), ("ctrl+shift+b", "Ctrl+Shift+B"),
            ("Shift+Ctrl+B", "Ctrl+Shift+B"), ("Cmd+K", "Meta+K"), ("Control+7", "Ctrl+7"), ("Alt+F9", "Alt+F9"),
            ("Ctrl + Shift + B", "Ctrl+Shift+B"), ("Meta+Alt+J", "Alt+Meta+J"),
        ] {
            assert_eq!(normalize(input).as_deref(), Ok(expected), "{input}");
        }
    }

    #[test]
    fn rejects_unsafe_or_malformed_values() {
        for input in [
            "", "+", "F25", "F0", "F01", "B", "7", "Shift+B", "Alt+B", "Space", "Ctrl+Space", "Ctrl+Enter", "Ctrl+Backspace",
            "Ctrl+Delete", "Ctrl+Tab", "Ctrl+ArrowUp", "Escape", "Ctrl+Escape", "Ctrl+Alt+B", "Ctrl+Ctrl+B", "Ctrl+",
            "Ctrl+Shift", "Hyper+B", "Ctrl+C", "Ctrl+V", "Ctrl+X", "Ctrl+A", "Ctrl+Z", "Ctrl+Shift+Z", "Meta+C", "Meta+Q",
            "Ctrl+W", "Ctrl+R", "Alt+F4", "Ctrl+ñ", "B+Ctrl",
        ] {
            assert!(normalize(input).is_err(), "debería rechazar «{input}»");
        }
    }

    #[test]
    fn default_is_valid_and_canonical() {
        assert_eq!(normalize(DEFAULT_SHORTCUT).unwrap(), DEFAULT_SHORTCUT);
    }

    #[test]
    fn normalization_is_idempotent() {
        for v in ["Ctrl+Shift+B", "F8", "Alt+Meta+J", "Meta+K"] {
            assert_eq!(normalize(&normalize(v).unwrap()).unwrap(), v);
        }
    }

    #[test]
    fn global_rules_follow_each_operating_system() {
        assert!(validate_global("Ctrl+Shift+B", "windows").is_ok());
        assert!(validate_global("Meta+Shift+B", "macos").is_ok());
        assert!(validate_global("Ctrl+Shift+B", "macos").is_ok());
        assert!(validate_global("Alt+F9", "windows").is_ok());
        assert!(validate_global("F12", "windows").is_ok());
        for bad in ["Meta+B", "Meta+Shift+B", "Alt+Meta+J"] {
            assert!(validate_global(bad, "windows").is_err(), "{bad} en Windows");
        }
        for bad in ["Meta+Q", "Meta+H", "Meta+M", "Meta+Shift+3", "Meta+Shift+4", "Meta+Shift+5", "Ctrl+Meta+Q", "Alt+Meta+D", "Meta+W"] {
            assert!(validate_global(bad, "macos").is_err(), "{bad} en macOS");
        }
        assert!(validate_global("Meta+Shift+Q", "macos").is_err());
        assert!(validate_global("Meta+K", "macos").is_ok());
        assert!(validate_global("Meta+Shift+7", "macos").is_ok());
    }

    #[test]
    fn plugin_strings_are_parseable_by_the_global_shortcut_plugin() {
        use std::str::FromStr;
        for v in ["F12", "F24", "Ctrl+Shift+B", "Alt+F9", "Ctrl+7", "Alt+Meta+J", "Meta+K", "Ctrl+Shift+Z"] {
            let text = to_plugin_string(v);
            assert!(tauri_plugin_global_shortcut::Shortcut::from_str(&text).is_ok(), "{text}");
        }
        assert_eq!(to_plugin_string("Meta+Shift+B"), "Super+Shift+B");
    }
}
