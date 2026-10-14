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
}
