//! Logique de raccourcis indépendante de la plateforme : table nom ↔ code virtuel Windows,
//! validation / détection de conflits, et automate de détection Push-to-Talk / Toggle
//! utilisé par le hook clavier bas niveau (`WH_KEYBOARD_LL`).

use std::collections::BTreeSet;

pub const VK_LBUTTON_MIDDLE: u32 = 0x04;
pub const VK_XBUTTON1: u32 = 0x05;
pub const VK_XBUTTON2: u32 = 0x06;
pub const VK_ESCAPE: u32 = 0x1B;

/// Table des touches supportées (codes `VK_*` Windows, distinction gauche/droite).
fn key_table() -> Vec<(String, u32)> {
    let mut t: Vec<(String, u32)> = vec![
        ("ControlLeft", 0xA2),
        ("ControlRight", 0xA3),
        ("ShiftLeft", 0xA0),
        ("ShiftRight", 0xA1),
        ("AltLeft", 0xA4),
        ("AltRight", 0xA5),
        ("MetaLeft", 0x5B),
        ("MetaRight", 0x5C),
        ("Space", 0x20),
        ("Enter", 0x0D),
        ("Tab", 0x09),
        ("Escape", 0x1B),
        ("Backspace", 0x08),
        ("CapsLock", 0x14),
        ("ScrollLock", 0x91),
        ("NumLock", 0x90),
        ("Pause", 0x13),
        ("PrintScreen", 0x2C),
        ("Insert", 0x2D),
        ("Delete", 0x2E),
        ("Home", 0x24),
        ("End", 0x23),
        ("PageUp", 0x21),
        ("PageDown", 0x22),
        ("ArrowLeft", 0x25),
        ("ArrowUp", 0x26),
        ("ArrowRight", 0x27),
        ("ArrowDown", 0x28),
        ("ContextMenu", 0x5D),
        ("NumpadMultiply", 0x6A),
        ("NumpadAdd", 0x6B),
        ("NumpadSubtract", 0x6D),
        ("NumpadDecimal", 0x6E),
        ("NumpadDivide", 0x6F),
        ("MouseMiddle", VK_LBUTTON_MIDDLE),
        ("Mouse4", VK_XBUTTON1),
        ("Mouse5", VK_XBUTTON2),
    ]
    .into_iter()
    .map(|(n, v)| (n.to_string(), v))
    .collect();
    for i in 0..24u32 {
        t.push((format!("F{}", i + 1), 0x70 + i));
    }
    for (i, c) in ('A'..='Z').enumerate() {
        t.push((format!("Key{c}"), 0x41 + i as u32));
    }
    for i in 0..10u32 {
        t.push((format!("Digit{i}"), 0x30 + i));
        t.push((format!("Numpad{i}"), 0x60 + i));
    }
    t
}

pub fn vk_from_name(name: &str) -> Option<u32> {
    let alias = match name {
        "AltGr" | "AltGraph" => "AltRight",
        "Control" | "Ctrl" => "ControlLeft",
        "Win" | "Meta" | "OSLeft" => "MetaLeft",
        "Alt" => "AltLeft",
        "Shift" => "ShiftLeft",
        "Return" => "Enter",
        other => other,
    };
    key_table()
        .into_iter()
        .find(|(n, _)| n == alias)
        .map(|(_, v)| v)
}

pub fn name_from_vk(vk: u32) -> Option<String> {
    // VK génériques (sans distinction gauche/droite) renvoyés par certains claviers.
    let vk = match vk {
        0x10 => 0xA0,
        0x11 => 0xA2,
        0x12 => 0xA4,
        v => v,
    };
    key_table().into_iter().find(|(_, v)| *v == vk).map(|(n, _)| n)
}

pub fn is_modifier(vk: u32) -> bool {
    matches!(vk, 0xA0..=0xA5 | 0x5B | 0x5C | 0x10 | 0x11 | 0x12)
}

pub fn is_mouse(vk: u32) -> bool {
    matches!(vk, VK_LBUTTON_MIDDLE | VK_XBUTTON1 | VK_XBUTTON2)
}

/// Famille d'une touche pour la comparaison avec les raccourcis système (gauche = droite).
fn family(vk: u32) -> u32 {
    match vk {
        0xA0 | 0xA1 => 0x10, // Shift
        0xA2 | 0xA3 => 0x11, // Ctrl
        0xA4 | 0xA5 => 0x12, // Alt
        0x5B | 0x5C => 0x5B, // Win
        v => v,
    }
}

/// Raccourcis système critiques à ne jamais détourner.
const CRITICAL: &[(&str, &[u32])] = &[
    ("Ctrl+C (copier)", &[0x11, 0x43]),
    ("Ctrl+V (coller)", &[0x11, 0x56]),
    ("Ctrl+X (couper)", &[0x11, 0x58]),
    ("Ctrl+Z (annuler)", &[0x11, 0x5A]),
    ("Ctrl+Y (rétablir)", &[0x11, 0x59]),
    ("Ctrl+A (tout sélectionner)", &[0x11, 0x41]),
    ("Ctrl+S (enregistrer)", &[0x11, 0x53]),
    ("Ctrl+F (rechercher)", &[0x11, 0x46]),
    ("Ctrl+W (fermer l'onglet)", &[0x11, 0x57]),
    ("Ctrl+T (nouvel onglet)", &[0x11, 0x54]),
    ("Ctrl+Tab", &[0x11, 0x09]),
    ("Ctrl+Échap (menu Démarrer)", &[0x11, 0x1B]),
    ("Alt+Tab", &[0x12, 0x09]),
    ("Alt+F4", &[0x12, 0x73]),
    ("Alt+Échap", &[0x12, 0x1B]),
    ("Ctrl+Alt+Suppr", &[0x11, 0x12, 0x2E]),
    ("Ctrl+Maj+Échap (gestionnaire des tâches)", &[0x11, 0x10, 0x1B]),
    ("Win+L (verrouiller)", &[0x5B, 0x4C]),
    ("Win+D (bureau)", &[0x5B, 0x44]),
    ("Win+E (explorateur)", &[0x5B, 0x45]),
    ("Win+R (exécuter)", &[0x5B, 0x52]),
    ("Win+V (historique presse-papiers)", &[0x5B, 0x56]),
    ("Win+Tab", &[0x5B, 0x09]),
    ("Win+X", &[0x5B, 0x58]),
    ("Win+I (paramètres)", &[0x5B, 0x49]),
    ("Win+H (dictée Windows)", &[0x5B, 0x48]),
    ("Win+Maj+S (capture)", &[0x5B, 0x10, 0x53]),
];

/// Retourne le nom du raccourci système en conflit, le cas échéant.
pub fn find_conflict(vks: &[u32]) -> Option<&'static str> {
    let fam: BTreeSet<u32> = vks.iter().map(|v| family(*v)).collect();
    CRITICAL
        .iter()
        .find(|(_, combo)| combo.iter().copied().collect::<BTreeSet<u32>>() == fam)
        .map(|(n, _)| *n)
}

/// Touches uniques autorisées seules (touches sous-utilisées).
fn allowed_single(vk: u32) -> bool {
    matches!(vk,
        0x70..=0x87          // F1-F24
        | 0x91 | 0x13 | 0x2D | 0x14 | 0x5D | 0x90 | 0x2C
        | 0xA3 | 0xA5 | 0x5C // Ctrl droit, AltGr, Win droit
        | VK_LBUTTON_MIDDLE | VK_XBUTTON1 | VK_XBUTTON2
        | 0x6A..=0x6F)
}

pub fn parse_combo(keys: &[String]) -> Result<Vec<u32>, String> {
    keys.iter()
        .map(|k| vk_from_name(k).ok_or_else(|| format!("Touche inconnue : {k}")))
        .collect()
}

pub fn validate_combo(keys: &[String]) -> Result<(), String> {
    if keys.is_empty() {
        return Err("Le raccourci est vide".into());
    }
    if keys.len() > 4 {
        return Err("Le raccourci ne peut contenir plus de 4 touches".into());
    }
    let vks = parse_combo(keys)?;
    let set: BTreeSet<u32> = vks.iter().copied().collect();
    if set.len() != vks.len() {
        return Err("Le raccourci contient des doublons".into());
    }
    if vks.contains(&VK_ESCAPE) {
        return Err("Échap est réservé à l'annulation".into());
    }
    if vks.len() == 1 && !allowed_single(vks[0]) {
        return Err(format!(
            "« {} » seule gênerait la saisie : choisissez une combinaison ou une touche sous-utilisée (F9, Ctrl droit, Verr. Défil…)",
            keys[0]
        ));
    }
    if vks.len() > 1 && vks.iter().all(|v| !is_modifier(*v)) {
        return Err("Une combinaison doit contenir au moins un modificateur (Ctrl, Alt, Maj, Win)".into());
    }
    if let Some(c) = find_conflict(&vks) {
        return Err(format!("Conflit avec un raccourci système critique : {c}"));
    }
    Ok(())
}

/// Ordonne une combinaison pour l'affichage : modificateurs d'abord.
pub fn normalize_names(vks: &[u32]) -> Vec<String> {
    let mut v: Vec<u32> = vks.to_vec();
    v.sort_by_key(|k| (!is_modifier(*k), *k));
    v.dedup();
    v.into_iter().filter_map(name_from_vk).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchEvent {
    None,
    Activated,
    Deactivated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyDecision {
    pub event: MatchEvent,
    /// Faut-il avaler l'événement (ne pas le transmettre à l'application) ?
    pub suppress: bool,
}

/// Automate de reconnaissance d'un raccourci (exact) à partir des événements bruts.
#[derive(Debug, Default)]
pub struct HotkeyMatcher {
    combo: BTreeSet<u32>,
    pressed: BTreeSet<u32>,
    active: bool,
    suppressed: BTreeSet<u32>,
}

impl HotkeyMatcher {
    pub fn new(combo: &[u32]) -> Self {
        Self {
            combo: combo.iter().copied().collect(),
            ..Default::default()
        }
    }

    pub fn set_combo(&mut self, combo: &[u32]) {
        self.combo = combo.iter().copied().collect();
        self.active = false;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn combo_has_meta(&self) -> bool {
        self.combo.iter().any(|v| family(*v) == 0x5B)
    }

    /// Réinitialise l'état (sortie de veille, réinstallation du hook).
    pub fn reset(&mut self) {
        self.pressed.clear();
        self.suppressed.clear();
        self.active = false;
    }

    pub fn on_key(&mut self, vk: u32, down: bool) -> KeyDecision {
        let vk = match vk {
            0x10 => 0xA0,
            0x11 => 0xA2,
            0x12 => 0xA4,
            v => v,
        };
        if self.combo.is_empty() {
            return KeyDecision { event: MatchEvent::None, suppress: false };
        }
        if down {
            self.pressed.insert(vk);
            let in_combo = self.combo.contains(&vk);
            if !self.active && self.pressed == self.combo {
                self.active = true;
                let suppress = in_combo && !is_modifier(vk);
                if suppress {
                    self.suppressed.insert(vk);
                }
                return KeyDecision { event: MatchEvent::Activated, suppress };
            }
            // Auto-répétition d'une touche du raccourci actif : avalée.
            if self.active && in_combo && !is_modifier(vk) {
                self.suppressed.insert(vk);
                return KeyDecision { event: MatchEvent::None, suppress: true };
            }
            KeyDecision { event: MatchEvent::None, suppress: false }
        } else {
            self.pressed.remove(&vk);
            let suppress = self.suppressed.remove(&vk);
            if self.active && self.combo.contains(&vk) {
                self.active = false;
                return KeyDecision { event: MatchEvent::Deactivated, suppress };
            }
            KeyDecision { event: MatchEvent::None, suppress }
        }
    }
}

/// Capture « Cliquer pour définir la touche » : enregistre toutes les touches enfoncées
/// jusqu'au relâchement complet.
#[derive(Debug, Default)]
pub struct KeyCapture {
    down: BTreeSet<u32>,
    seen: Vec<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CaptureResult {
    Pending,
    Cancelled,
    Done(Vec<String>),
}

impl KeyCapture {
    pub fn on_key(&mut self, vk: u32, down: bool) -> CaptureResult {
        let vk = match vk {
            0x10 => 0xA0,
            0x11 => 0xA2,
            0x12 => 0xA4,
            v => v,
        };
        if down {
            if vk == VK_ESCAPE && self.seen.is_empty() {
                return CaptureResult::Cancelled;
            }
            self.down.insert(vk);
            if !self.seen.contains(&vk) {
                self.seen.push(vk);
            }
            CaptureResult::Pending
        } else {
            self.down.remove(&vk);
            if self.down.is_empty() && !self.seen.is_empty() {
                CaptureResult::Done(normalize_names(&self.seen))
            } else {
                CaptureResult::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn names_roundtrip() {
        for n in ["ControlLeft", "Space", "F9", "ScrollLock", "KeyA", "Digit5", "Mouse4", "AltRight"] {
            let vk = vk_from_name(n).unwrap();
            assert_eq!(name_from_vk(vk).unwrap(), n);
        }
        assert_eq!(vk_from_name("AltGr"), Some(0xA5));
    }

    #[test]
    fn validation_rules() {
        assert!(validate_combo(&s(&["ControlLeft", "Space"])).is_ok());
        assert!(validate_combo(&s(&["MetaLeft", "AltLeft"])).is_ok());
        assert!(validate_combo(&s(&["ControlRight"])).is_ok());
        assert!(validate_combo(&s(&["F9"])).is_ok());
        assert!(validate_combo(&s(&["ScrollLock"])).is_ok());
        assert!(validate_combo(&s(&["Mouse4"])).is_ok());
        assert!(validate_combo(&s(&["AltGr"])).is_ok());
        assert!(validate_combo(&s(&["KeyA"])).is_err());
        assert!(validate_combo(&s(&["Space"])).is_err());
        assert!(validate_combo(&s(&["Escape"])).is_err());
        assert!(validate_combo(&s(&["ControlLeft", "KeyC"])).unwrap_err().contains("Ctrl+C"));
        assert!(validate_combo(&s(&["ControlRight", "KeyV"])).is_err());
        assert!(validate_combo(&s(&["MetaLeft", "KeyL"])).is_err());
        assert!(validate_combo(&s(&["AltLeft", "Tab"])).is_err());
        assert!(validate_combo(&s(&["KeyA", "KeyB"])).is_err());
        assert!(validate_combo(&s(&["Nope"])).is_err());
        assert!(validate_combo(&[]).is_err());
    }

    #[test]
    fn push_to_talk_sequence() {
        let mut m = HotkeyMatcher::new(&[0xA2, 0x20]);
        assert_eq!(m.on_key(0xA2, true).event, MatchEvent::None);
        let d = m.on_key(0x20, true);
        assert_eq!(d, KeyDecision { event: MatchEvent::Activated, suppress: true });
        // auto-répétition
        assert_eq!(m.on_key(0x20, true), KeyDecision { event: MatchEvent::None, suppress: true });
        let u = m.on_key(0x20, false);
        assert_eq!(u, KeyDecision { event: MatchEvent::Deactivated, suppress: true });
        assert_eq!(m.on_key(0xA2, false), KeyDecision { event: MatchEvent::None, suppress: false });
    }

    #[test]
    fn exact_match_only() {
        let mut m = HotkeyMatcher::new(&[0xA2, 0x20]);
        m.on_key(0xA2, true);
        m.on_key(0xA0, true); // Shift en plus
        assert_eq!(m.on_key(0x20, true).event, MatchEvent::None);
    }

    #[test]
    fn single_modifier_key() {
        let mut m = HotkeyMatcher::new(&[0xA3]);
        assert_eq!(m.on_key(0xA3, true), KeyDecision { event: MatchEvent::Activated, suppress: false });
        assert_eq!(m.on_key(0xA3, false).event, MatchEvent::Deactivated);
        // Ctrl gauche ne déclenche pas Ctrl droit
        assert_eq!(m.on_key(0xA2, true).event, MatchEvent::None);
    }

    #[test]
    fn capture_combo() {
        let mut c = KeyCapture::default();
        assert_eq!(c.on_key(0x20, true), CaptureResult::Pending);
        assert_eq!(c.on_key(0xA2, true), CaptureResult::Pending);
        assert_eq!(c.on_key(0x20, false), CaptureResult::Pending);
        assert_eq!(
            c.on_key(0xA2, false),
            CaptureResult::Done(vec!["ControlLeft".into(), "Space".into()])
        );
        let mut c = KeyCapture::default();
        assert_eq!(c.on_key(VK_ESCAPE, true), CaptureResult::Cancelled);
    }
}
