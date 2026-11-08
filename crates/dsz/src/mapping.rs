//! Mapping policies shared by the editor and the input pipeline.
use std::time::{Duration, Instant};

use ds_proto::input::Button;
use serde::{Deserialize, Serialize};

use crate::profile::{Action, Mapping, Profile, VirtualKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModifierMode {
    #[default]
    Timed,
    Dedicated,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Modifier {
    pub button: Button,
    pub mode: ModifierMode,
    pub wait_ms: u32,
}

impl Default for Modifier {
    fn default() -> Self {
        Self { button: Button::R3, mode: ModifierMode::Timed, wait_ms: 200 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CopyPolicy {
    Replace,
    KeepBoth,
    #[default]
    Skip,
}

fn mask(m: &Mapping) -> u32 {
    m.buttons().fold(0, |bits, b| bits | b.bit())
}

pub fn same_trigger(a: &Mapping, b: &Mapping) -> bool {
    mask(a) == mask(b) && a.gesture == b.gesture
}

pub fn navigation_session(profile: &Profile, mapping: &Mapping) -> Option<u32> {
    navigation_prefix(&profile.mappings, mapping)
}

fn navigation_prefix(mappings: &[Mapping], mapping: &Mapping) -> Option<u32> {
    let nav = mask(mapping);
    mappings.iter().filter(|hold| hold.enabled && hold.action == Action::AppSwitcherHold
        && hold.button == mapping.button && mask(hold) != nav && mask(hold) & nav == mask(hold))
        .map(mask).max_by_key(|bits| bits.count_ones())
}

pub fn undo_copy(profile: &mut Profile, before: &[Mapping], after: &[Mapping]) -> bool {
    if profile.mappings != after { return false; }
    profile.mappings = before.to_vec();
    true
}

pub fn copy_mapping(profile: &mut Profile, mapping: &Mapping, policy: CopyPolicy) -> bool {
    if profile.mappings.iter().any(|m| same_trigger(m, mapping)) {
        match policy {
            CopyPolicy::Skip => return false,
            CopyPolicy::Replace => profile.mappings.retain(|m| !same_trigger(m, mapping)),
            CopyPolicy::KeepBoth => {}
        }
    }
    profile.mappings.push(mapping.clone());
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarningKind { Duplicate, Overlap, Solo, Gesture, Leakage, NoSession, NoVirtual }

pub fn warnings(profile: &Profile) -> Vec<(usize, Option<usize>, WarningKind)> {
    let mut out = Vec::new();
    for (i, m) in profile.mappings.iter().enumerate().filter(|(_, m)| m.enabled) {
        if m.is_combo() && m.mute {
            // Timed modifiers can still pass through before a late combo.
            let dedicated = |b| profile.modifiers.iter().any(|r| r.button == b && r.mode == ModifierMode::Dedicated);
            if m.buttons().any(|b| !dedicated(b)) {
                out.push((i, None, WarningKind::Leakage));
            }
        }
        if matches!(m.action, Action::AppSwitcherNext | Action::AppSwitcherPrevious) {
            if navigation_session(profile, m).is_none() { out.push((i, None, WarningKind::NoSession)); }
        }
        if profile.virtual_out.kind == VirtualKind::Off && (m.mute || !profile.modifiers.is_empty()) {
            out.push((i, None, WarningKind::NoVirtual));
        }
        for (j, n) in profile.mappings.iter().enumerate().skip(i + 1).filter(|(_, n)| n.enabled) {
            let a = mask(m); let b = mask(n);
            if (m.action == Action::AppSwitcherHold && navigation_session(profile, n) == Some(a))
                || (n.action == Action::AppSwitcherHold && navigation_session(profile, m) == Some(b)) { continue; }
            if same_trigger(m, n) {
                out.push((i, Some(j), WarningKind::Duplicate));
            } else if a == b {
                out.push((i, Some(j), WarningKind::Gesture));
            } else if a & b != 0 {
                let kind = if m.is_combo() != n.is_combo() { WarningKind::Solo } else { WarningKind::Overlap };
                out.push((i, Some(j), kind));
            }
        }
    }
    out
}

#[derive(Default)]
struct ModifierState {
    since: Option<Instant>,
    consumed: bool,
    forwarded: bool,
    tap_until: Option<Instant>,
}

#[derive(Default)]
pub struct Modifiers {
    states: Vec<ModifierState>,
}

#[derive(Default, Debug)]
pub struct ModifierOutput {
    pub suppress: u32,
    pub inject: u32,
}

impl Modifiers {
    pub fn update(&mut self, rules: &[Modifier], mappings: &[Mapping], buttons: u32, now: Instant) -> ModifierOutput {
        self.states.resize_with(rules.len(), ModifierState::default);
        let mut out = ModifierOutput::default();
        for (rule, state) in rules.iter().zip(self.states.iter_mut()) {
            let bit = rule.button.bit();
            if buttons & bit != 0 {
                let since = *state.since.get_or_insert(now);
                state.tap_until = None;
                let combo = mappings.iter().any(|m| m.enabled && m.is_combo() && m.action != Action::AppSwitcherHold
                    && (!matches!(m.action, Action::AppSwitcherNext | Action::AppSwitcherPrevious)
                        || navigation_prefix(mappings, m).is_some())
                    && mask(m) & bit != 0 && buttons & mask(m) == mask(m));
                state.consumed |= combo;
                if state.consumed || rule.mode == ModifierMode::Dedicated
                    || now.duration_since(since) < Duration::from_millis(rule.wait_ms.clamp(50, 1000) as u64)
                {
                    out.suppress |= bit;
                } else {
                    state.forwarded = true;
                }
            } else {
                if state.since.take().is_some() {
                    if !state.consumed && !state.forwarded && rule.mode == ModifierMode::Timed {
                        state.tap_until = Some(now + Duration::from_millis(30));
                    }
                    state.consumed = false;
                    state.forwarded = false;
                }
                if state.tap_until.is_some_and(|t| now < t) { out.inject |= bit; }
            }
        }
        out
    }
}

#[derive(Default)]
pub struct AppSwitcher {
    held: u32,
    owner: u32,
    alt_down: bool,
}

impl AppSwitcher {
    pub fn arm(&mut self, held: u32, mut send: impl FnMut(u16, bool)) {
        if self.owner & held != self.owner && self.alt_down {
            send(0xA4, false);
            self.alt_down = false;
        }
        self.held = held;
    }

    pub fn step(&mut self, prefix: u32, previous: bool, mut send: impl FnMut(u16, bool)) {
        if prefix == 0 || self.held & prefix != prefix { return; }
        if self.alt_down && self.owner != prefix { send(0xA4, false); self.alt_down = false; }
        self.owner = prefix;
        if !self.alt_down { send(0xA4, true); self.alt_down = true; }
        if previous { send(0xA0, true); }
        send(9, true);
        send(9, false);
        if previous { send(0xA0, false); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo() -> Mapping {
        Mapping { button: Button::R1, with: vec![Button::R3], mute: true,
            action: Action::Media { key: crate::profile::MediaKey::PlayPause }, ..Default::default() }
    }

    #[test]
    fn timed_modifier_replays_unused_tap_and_preserves_a_normal_hold() {
        let now = Instant::now();
        let rules = [Modifier::default()];
        let b = Button::R3.bit();
        let mut rt = Modifiers::default();
        assert_eq!(rt.update(&rules, &[], b, now).suppress, b);
        assert_eq!(rt.update(&rules, &[], 0, now + Duration::from_millis(50)).inject, b);
        assert_eq!(rt.update(&rules, &[], 0, now + Duration::from_millis(90)).inject, 0);
        assert_eq!(rt.update(&rules, &[], b, now + Duration::from_secs(1)).suppress, b);
        assert_eq!(rt.update(&rules, &[], b, now + Duration::from_millis(1201)).suppress, 0);
        assert_eq!(rt.update(&rules, &[], 0, now + Duration::from_millis(1400)).inject, 0);
    }

    #[test]
    fn late_combo_consumes_until_release_without_replaying_r3() {
        let now = Instant::now();
        let rules = [Modifier::default()];
        let maps = [combo()];
        let b = Button::R3.bit();
        let mut rt = Modifiers::default();
        rt.update(&rules, &maps, b, now);
        assert_eq!(rt.update(&rules, &maps, b, now + Duration::from_millis(250)).suppress, 0);
        assert_eq!(rt.update(&rules, &maps, b | Button::R1.bit(), now + Duration::from_millis(300)).suppress, b);
        assert_eq!(rt.update(&rules, &maps, b, now + Duration::from_millis(350)).suppress, b);
        assert_eq!(rt.update(&rules, &maps, 0, now + Duration::from_millis(400)).inject, 0);
    }

    #[test]
    fn dedicated_modifier_never_passes_through_and_disabled_combos_do_not_consume() {
        let now = Instant::now();
        let b = Button::R3.bit();
        let rules = [Modifier { mode: ModifierMode::Dedicated, ..Default::default() }];
        let mut rt = Modifiers::default();
        assert_eq!(rt.update(&rules, &[], b, now).suppress, b);
        assert_eq!(rt.update(&rules, &[], b, now + Duration::from_secs(2)).suppress, b);
        assert_eq!(rt.update(&rules, &[], 0, now + Duration::from_secs(3)).inject, 0);
        let mut m = combo(); m.enabled = false;
        let mut rt = Modifiers::default();
        rt.update(&[Modifier::default()], &[m.clone()], b | Button::R1.bit(), now);
        assert_eq!(rt.update(&[Modifier::default()], &[m], 0, now + Duration::from_millis(50)).inject, b);
    }

    #[test]
    fn no_modifier_rules_leave_input_unchanged() {
        let out = Modifiers::default().update(&[], &[combo()], u32::MAX, Instant::now());
        assert_eq!((out.suppress, out.inject), (0, 0));
        let p: Profile = serde_json::from_str(r#"{"name":"Old"}"#).unwrap();
        assert!(p.modifiers.is_empty());
    }

    #[test]
    fn copy_preserves_every_setting_and_resolves_reordered_duplicate_combos() {
        let m = combo();
        let mut p = Profile::default();
        assert!(copy_mapping(&mut p, &m, CopyPolicy::Skip));
        assert_eq!(p.mappings, [m.clone()]);
        let mut flipped = m.clone(); flipped.button = Button::R3; flipped.with = vec![Button::R1];
        flipped.turbo_ms = 321;
        assert!(!copy_mapping(&mut p, &flipped, CopyPolicy::Skip));
        assert!(copy_mapping(&mut p, &flipped, CopyPolicy::KeepBoth));
        assert_eq!(p.mappings.len(), 2);
        assert!(copy_mapping(&mut p, &flipped, CopyPolicy::Replace));
        assert_eq!(p.mappings, [flipped]);
    }

    #[test]
    fn warnings_ignore_disabled_rows_but_find_overlap_duplicates_and_leaks() {
        let mut p = Profile::default();
        p.mappings = vec![combo(), combo(), Mapping { button: Button::R3, ..Default::default() }];
        let w = warnings(&p);
        assert!(w.iter().any(|w| w.2 == WarningKind::Duplicate));
        assert!(w.iter().any(|w| w.2 == WarningKind::Solo));
        assert!(w.iter().any(|w| w.2 == WarningKind::Leakage));
        p.mappings[1].with.push(Button::L1);
        assert!(warnings(&p).iter().any(|w| w.2 == WarningKind::Overlap));
        for m in &mut p.mappings { m.enabled = false; }
        assert!(warnings(&p).is_empty());
    }

    #[test]
    fn app_switcher_keeps_alt_across_steps_and_releases_it_on_cancel() {
        let mut s = AppSwitcher::default();
        let mut events = Vec::new();
        s.arm(Button::R3.bit(), |k,d| events.push((k,d)));
        assert!(events.is_empty());
        s.step(Button::R3.bit(), false, |k,d| events.push((k,d)));
        s.step(Button::R3.bit(), true, |k,d| events.push((k,d)));
        s.arm(0, |k,d| events.push((k,d)));
        assert_eq!(events, [(0xA4,true), (9,true), (9,false), (0xA0,true), (9,true), (9,false), (0xA0,false), (0xA4,false)]);
        s.step(Button::R3.bit(), false, |k,d| events.push((k,d)));
        assert_eq!(events.len(), 8);
    }

    #[test]
    fn app_switcher_prefix_can_have_multiple_buttons_but_navigation_needs_an_extra() {
        let hold = Mapping { button: Button::R3, with: vec![Button::L2], action: Action::AppSwitcherHold, ..Default::default() };
        let mut nav = Mapping { button: Button::R3, with: vec![Button::L2, Button::R1], action: Action::AppSwitcherNext, ..Default::default() };
        let mut profile = Profile::default(); profile.mappings = vec![hold.clone(), nav.clone()];
        let prefix = Button::R3.bit() | Button::L2.bit();
        assert_eq!(navigation_session(&profile, &nav), Some(prefix));
        nav.with = vec![Button::L2];
        assert_eq!(navigation_session(&profile, &nav), None);
        nav.with = vec![Button::R1];
        assert_eq!(navigation_session(&profile, &nav), None);
        nav.button = Button::R1; nav.with = vec![Button::R3, Button::L2];
        assert_eq!(navigation_session(&profile, &nav), None, "the base must match too");
        let rules = [Modifier::default()];
        let now = Instant::now(); let mut rt = Modifiers::default();
        rt.update(&rules, &profile.mappings, prefix, now);
        assert_eq!(rt.update(&rules, &profile.mappings, 0, now + Duration::from_millis(50)).inject, Button::R3.bit(), "arming alone does not consume the modifier");
        let mut switcher = AppSwitcher::default(); let mut events = Vec::new();
        switcher.arm(prefix, |k,d| events.push((k,d)));
        switcher.step(prefix, false, |k,d| events.push((k,d)));
        switcher.arm(Button::R3.bit(), |k,d| events.push((k,d)));
        assert_eq!(events.last(), Some(&(0xA4, false)), "release any prefix button to commit");
    }

    #[test]
    fn undo_copy_keeps_other_profile_settings_and_refuses_later_mapping_edits() {
        let mut p = Profile::default(); let before = p.mappings.clone();
        copy_mapping(&mut p, &combo(), CopyPolicy::KeepBoth);
        let after = p.mappings.clone();
        p.modifiers.push(Modifier::default());
        assert!(undo_copy(&mut p, &before, &after));
        assert_eq!(p.modifiers.len(), 1);
        p.mappings = after.clone(); p.mappings[0].mute = false;
        assert!(!undo_copy(&mut p, &before, &after));
        assert_eq!(p.mappings.len(), 1);
    }

    #[test]
    fn multi_button_navigation_selects_the_longest_prefix_and_rearms_cleanly() {
        let mut p = Profile::default();
        p.mappings = vec![Mapping { button: Button::R3, action: Action::AppSwitcherHold, ..Default::default() },
            Mapping { button: Button::R3, with: vec![Button::L2], action: Action::AppSwitcherHold, ..Default::default() }];
        let nav = Mapping { button: Button::R3, with: vec![Button::L2, Button::R1, Button::Cross], action: Action::AppSwitcherNext, ..Default::default() };
        let prefix = Button::R3.bit() | Button::L2.bit();
        assert_eq!(navigation_session(&p, &nav), Some(prefix));
        let mut rt = AppSwitcher::default(); let mut events = Vec::new();
        rt.arm(prefix, |k,d| events.push((k,d)));
        rt.step(prefix, true, |k,d| events.push((k,d)));
        rt.arm(0, |k,d| events.push((k,d)));
        rt.arm(prefix, |k,d| events.push((k,d)));
        rt.step(prefix, false, |k,d| events.push((k,d)));
        assert_eq!(events.iter().filter(|e| **e == (0xA4,true)).count(), 2);
        assert_eq!(events.iter().filter(|e| **e == (0xA0,false)).count(), 1);
    }
}
