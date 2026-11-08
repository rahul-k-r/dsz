use eframe::egui::{self, CornerRadius, RichText, Stroke};

use ds_proto::input::Button;

use super::button_choices;
use crate::inputsim::{key_catalog, key_name, vk_from_egui};
use crate::profile::*;
use crate::mapping::{self, CopyPolicy, Modifier, ModifierMode, WarningKind};
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::ui::App;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AKind {
    None,
    Key,
    Mouse,
    ScrollUp,
    ScrollDown,
    Media,
    Next,
    Prev,
    Gyro,
    Touch,
    Run,
    Profile,
    HoldProfile,
    Gamepad,
    Xbox,
    TurnOff,
    SwitchHold,
    SwitchNext,
    SwitchPrevious,
}

fn akind(a: &Action) -> AKind {
    match a {
        Action::None => AKind::None,
        Action::AppSwitcherHold => AKind::SwitchHold,
        Action::AppSwitcherNext => AKind::SwitchNext,
        Action::AppSwitcherPrevious => AKind::SwitchPrevious,
        Action::Key { .. } => AKind::Key,
        Action::Mouse { .. } => AKind::Mouse,
        Action::ScrollUp => AKind::ScrollUp,
        Action::ScrollDown => AKind::ScrollDown,
        Action::Media { .. } => AKind::Media,
        Action::NextProfile => AKind::Next,
        Action::PreviousProfile => AKind::Prev,
        Action::ToggleGyroMouse => AKind::Gyro,
        Action::ToggleTouchpadMouse => AKind::Touch,
        Action::Run { .. } => AKind::Run,
        Action::Profile { .. } => AKind::Profile,
        Action::HoldProfile { .. } => AKind::HoldProfile,
        Action::Gamepad { .. } => AKind::Gamepad,
        Action::Xbox { .. } => AKind::Xbox,
        Action::TurnOffController => AKind::TurnOff,
    }
}

const KINDS: [(AKind, &str); 19] = [
    (AKind::SwitchHold, "App switcher: hold"),
    (AKind::SwitchNext, "App switcher: next"),
    (AKind::SwitchPrevious, "App switcher: previous"),
    (AKind::None, "Nothing"),
    (AKind::Gamepad, "PlayStation button"),
    (AKind::Xbox, "Xbox button"),
    (AKind::Key, "Keyboard key"),
    (AKind::Mouse, "Mouse button"),
    (AKind::ScrollUp, "Scroll up"),
    (AKind::ScrollDown, "Scroll down"),
    (AKind::Media, "Media key"),
    (AKind::Next, "Next profile"),
    (AKind::Prev, "Previous profile"),
    (AKind::Gyro, "Toggle gyro"),
    (AKind::Touch, "Toggle touchpad mouse"),
    (AKind::Run, "Open a program / file"),
    (AKind::Profile, "Switch to profile"),
    (AKind::HoldProfile, "Use profile while held"),
    (AKind::TurnOff, "Turn off controller"),
];

fn mouse_label(b: MouseButton) -> &'static str {
    match b {
        MouseButton::Left => crate::i18n::tr("Left"),
        MouseButton::Right => crate::i18n::tr("Right"),
        MouseButton::Middle => crate::i18n::tr("Middle"),
        MouseButton::Back => crate::i18n::tr("Back"),
        MouseButton::Forward => crate::i18n::tr("Forward"),
    }
}

fn key_label(vk: u16) -> String {
    crate::i18n::tr(&key_name(vk)).to_string()
}

impl App {
    pub(crate) fn page_mappings(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        p: &mut Profile,
    ) {
        page_header(
            ui,
            crate::i18n::tr("Button mapping"),
            crate::i18n::tr("Send keys, mouse buttons, media keys, or app actions from any button or combo, including turning a Bluetooth controller off. Touchpad Left, Center, and Right are the click in each third of the pad. Touchpad is still the whole click. With a virtual controller on, a mapping can also hide its button from the game or press a different controller button. Xbox buttons work on any profile, so the Xbox button can open Game Bar (tap) or Task View (hold)."),
        );
        let edge = self
            .engine
            .device_list()
            .first()
            .map(|d| d.model.is_edge())
            .unwrap_or(true);
        let virtual_on = p.virtual_out.kind != VirtualKind::Off;
        let xbox_on = p.virtual_out.kind == VirtualKind::Xbox360;
        let can_hide = self.engine.companion.can_hide();
        let profile_names = self.engine.profiles.names();
        self.mapping_copy_editor(ui, p);
        card(ui, crate::i18n::tr("Modifier buttons"), "", |ui| {
            muted(ui, crate::i18n::tr("Timed modifiers wait for a combo, replay unused taps, and pass normal holds through after the delay. A later combo suppresses them until release. Dedicated modifiers never reach the game. Requires a virtual controller."));
            let chosen: Vec<Button> = p.modifiers.iter().map(|r| r.button).collect();
            let mut remove_modifier = None;
            for (i, rule) in p.modifiers.iter_mut().enumerate() {
                ui.push_id(("modifier", i), |ui| ui.horizontal_wrapped(|ui| {
                    fit(ui, 110.0);
                    egui::ComboBox::from_id_salt("button").width(100.0).selected_text(crate::i18n::tr(rule.button.short_name())).show_ui(ui, |ui| {
                        for b in button_choices(edge).into_iter().filter(|b| !b.is_touchpad_zone()) {
                            if b == rule.button || !chosen.contains(&b) { ui.selectable_value(&mut rule.button, b, crate::i18n::tr(b.short_name())); }
                        }
                    });
                    fit(ui, 310.0);
                    segmented(ui, &mut rule.mode, &[(ModifierMode::Timed, crate::i18n::tr("Timed pass-through")), (ModifierMode::Dedicated, crate::i18n::tr("Dedicated modifier"))]);
                    if rule.mode == ModifierMode::Timed {
                        fit(ui, 420.0);
                        ui.add(egui::Slider::new(&mut rule.wait_ms, 50..=1000).suffix(" ms").text(crate::i18n::tr("Wait for combo")));
                    }
                    if button(ui, crate::i18n::tr("Remove")).clicked() { remove_modifier = Some(i); }
                }));
            }
            if let Some(i) = remove_modifier { p.modifiers.remove(i); }
            if let Some(b) = std::iter::once(Button::R3).chain(button_choices(edge)).find(|b| !b.is_touchpad_zone() && !p.modifiers.iter().any(|r| r.button == *b)) {
                if button(ui, crate::i18n::tr("+ Add modifier")).clicked() { p.modifiers.push(Modifier { button: b, ..Default::default() }); }
            }
        });
        ui.add_space(10.0);
        let warnings = mapping::warnings(p);

        // Key capture.
        if let Some(i) = self.capture_key {
            let got = ctx.input(|inp| {
                inp.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((k, m)) = got {
                if let (Some(vk), Some(map)) = (vk_from_egui(k), p.mappings.get_mut(i)) {
                    map.action = Action::Key {
                        vk,
                        ctrl: m.ctrl,
                        shift: m.shift,
                        alt: m.alt,
                        win: false,
                    };
                }
                self.capture_key = None;
            }
        }

        let mut remove: Option<usize> = None;
        let mut capture: Option<Option<usize>> = None;
        let mut copy_request = None;
        if p.mappings.is_empty() {
            card(ui, "", "", |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new(crate::i18n::tr("No mappings yet")).font(heading(16.0)));
                    muted(
                        ui,
                        crate::i18n::tr("Add one to turn a button into a key press, a click, or a shortcut."),
                    );
                    ui.add_space(10.0);
                });
            });
            ui.add_space(10.0);
        }
        let row_w = (ui.available_width() - 34.0).max(100.0);
        let p_name = p.name.clone();
        for (i, m) in p.mappings.iter_mut().enumerate() {
            egui::Frame::new()
                .fill(CARD)
                .stroke(Stroke::new(1.0_f32, if m.enabled { BORDER } else { with_alpha(BORDER, 120) }))
                .corner_radius(CornerRadius::same(R_CARD))
                .inner_margin(egui::Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    ui.set_width(row_w);
                    // The remove button sits in the top-right corner, out of
                    // the wrapped row, so a narrow window wraps the row
                    // instead of pushing the card wider.
                    let top = ui.cursor().min;
                    let x = egui::Rect::from_min_size(
                        egui::pos2(top.x + row_w - 20.0, top.y),
                        egui::vec2(20.0, 28.0),
                    );
                    let xr = ui.interact(x, ui.id().with(("map-x", i)), egui::Sense::click()).on_hover_text(crate::i18n::tr("Remove"));
                    ui.painter().text(
                        x.center(),
                        egui::Align2::CENTER_CENTER,
                        "✕",
                        egui::FontId::proportional(14.0),
                        if xr.hovered() { TEXT } else { MUTED },
                    );
                    if xr.clicked() {
                        remove = Some(i);
                    }
                    ui.scope(|ui| {
                    ui.set_max_width(row_w - 28.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("#{}", i + 1)).color(MUTED));
                        toggle(ui, &mut m.enabled);
                        fit(ui, 130.0);
                        egui::ComboBox::from_id_salt(("map-btn", i))
                            .width(130.0)
                            .selected_text(RichText::new(crate::i18n::tr(m.button.short_name())).strong())
                            .show_ui(ui, |ui| {
                                for b in button_choices(edge) {
                                    ui.selectable_value(&mut m.button, b, crate::i18n::tr(b.short_name()));
                                }
                            });
                        fit(ui, 120.0);
                        egui::ComboBox::from_id_salt(("map-act", i))
                            .width(120.0)
                            .selected_text(match m.gesture {
                                Gesture::Press => crate::i18n::tr("Press"),
                                Gesture::LongPress => crate::i18n::tr("Long press"),
                                Gesture::DoubleTap => crate::i18n::tr("Double tap"),
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut m.gesture, Gesture::Press, crate::i18n::tr("Press"));
                                ui.selectable_value(&mut m.gesture, Gesture::LongPress, crate::i18n::tr("Long press"));
                                ui.selectable_value(&mut m.gesture, Gesture::DoubleTap, crate::i18n::tr("Double tap"));
                            });
                        ui.label(RichText::new("→").color(MUTED).size(16.0));
                        let mut k = akind(&m.action);
                        let before = k;
                        fit(ui, 170.0);
                        egui::ComboBox::from_id_salt(("map-kind", i))
                            .width(170.0)
                            .selected_text(crate::i18n::tr(KINDS.iter().find(|x| x.0 == k).map(|x| x.1).unwrap_or("")))
                            .show_ui(ui, |ui| {
                                for (kk, l) in KINDS {
                                    ui.selectable_value(&mut k, kk, crate::i18n::tr(l));
                                }
                            });
                        if k != before {
                            m.action = match k {
                                AKind::None => Action::None,
                                AKind::SwitchHold => { m.gesture = Gesture::Press; m.mode = HoldStyle::Hold; m.mute = false; Action::AppSwitcherHold },
                                AKind::SwitchNext => { m.gesture = Gesture::Press; m.mode = HoldStyle::Tap; m.mute = true; Action::AppSwitcherNext },
                                AKind::SwitchPrevious => { m.gesture = Gesture::Press; m.mode = HoldStyle::Tap; m.mute = true; Action::AppSwitcherPrevious },
                                AKind::Key => Action::Key { vk: 0x20, ctrl: false, shift: false, alt: false, win: false },
                                AKind::Mouse => Action::Mouse { button: MouseButton::Left },
                                AKind::ScrollUp => Action::ScrollUp,
                                AKind::ScrollDown => Action::ScrollDown,
                                AKind::Media => Action::Media { key: MediaKey::PlayPause },
                                AKind::Next => Action::NextProfile,
                                AKind::Prev => Action::PreviousProfile,
                                AKind::Gyro => Action::ToggleGyroMouse,
                                AKind::Touch => Action::ToggleTouchpadMouse,
                                AKind::Run => Action::Run { path: String::new() },
                                AKind::TurnOff => Action::TurnOffController,
                                AKind::Profile => Action::Profile {
                                    name: profile_names.iter().find(|n| **n != p_name).cloned().unwrap_or_default(),
                                },
                                AKind::HoldProfile => {
                                    // A held button usually shouldn't reach the game too.
                                    m.mute = true;
                                    Action::HoldProfile {
                                        name: profile_names.iter().find(|n| **n != p_name).cloned().unwrap_or_default(),
                                        then: String::new(),
                                    }
                                }
                                AKind::Gamepad => Action::Gamepad { button: m.button.physical() },
                                AKind::Xbox => Action::Xbox {
                                    button: XboxPadButton::ALL
                                        .into_iter()
                                        .find(|x| x.pad_button() == m.button)
                                        .unwrap_or(XboxPadButton::Guide),
                                },
                            };
                        }
                        match &mut m.action {
                            Action::Key { vk, ctrl, shift, alt, win } => {
                                fit(ui, 110.0);
                                egui::ComboBox::from_id_salt(("map-key", i))
                                    .width(110.0)
                                    .selected_text(key_label(*vk))
                                    .height(320.0)
                                    .show_ui(ui, |ui| {
                                        for c in key_catalog() {
                                            ui.selectable_value(vk, c, key_label(c));
                                        }
                                    });
                                let rec = self.capture_key == Some(i);
                                if ui
                                    .add(egui::Button::new(if rec { crate::i18n::tr("Press a key…") } else { crate::i18n::tr("Record") }).fill(if rec { ACCENT } else { CARD_HI }))
                                    .clicked()
                                {
                                    capture = Some(if rec { None } else { Some(i) });
                                }
                                ui.checkbox(ctrl, "Ctrl");
                                ui.checkbox(shift, crate::i18n::tr("Shift"));
                                ui.checkbox(alt, "Alt");
                                ui.checkbox(win, "Win");
                            }
                            Action::Mouse { button } => {
                                fit(ui, 110.0);
                                egui::ComboBox::from_id_salt(("map-mouse", i))
                                    .width(110.0)
                                    .selected_text(mouse_label(*button))
                                    .show_ui(ui, |ui| {
                                        for b in [MouseButton::Left, MouseButton::Right, MouseButton::Middle, MouseButton::Back, MouseButton::Forward] {
                                            ui.selectable_value(button, b, mouse_label(b));
                                        }
                                    });
                            }
                            Action::Media { key } => {
                                fit(ui, 150.0);
                                egui::ComboBox::from_id_salt(("map-media", i))
                                    .width(150.0)
                                    .selected_text(crate::i18n::tr(key.label()))
                                    .show_ui(ui, |ui| {
                                        for k in MediaKey::ALL {
                                            ui.selectable_value(key, k, crate::i18n::tr(k.label()));
                                        }
                                    });
                            }
                            Action::Profile { name } => {
                                fit(ui, 170.0);
                                egui::ComboBox::from_id_salt(("map-prof", i))
                                    .width(170.0)
                                    .selected_text(if name.is_empty() { crate::i18n::tr("Choose…") } else { name.as_str() })
                                    .show_ui(ui, |ui| {
                                        for n in &profile_names {
                                            ui.selectable_value(name, n.clone(), n);
                                        }
                                    });
                            }
                            Action::HoldProfile { name, then } => {
                                fit(ui, 170.0);
                                egui::ComboBox::from_id_salt(("map-hold", i))
                                    .width(170.0)
                                    .selected_text(if name.is_empty() { crate::i18n::tr("Choose…") } else { name.as_str() })
                                    .show_ui(ui, |ui| {
                                        for n in &profile_names {
                                            ui.selectable_value(name, n.clone(), n);
                                        }
                                    });
                                fit(ui, 250.0);
                                // The label stays with its list when the row wraps.
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(crate::i18n::tr("on release")).size(12.5).color(MUTED));
                                    egui::ComboBox::from_id_salt(("map-hold-then", i))
                                        .width(170.0)
                                        .selected_text(if then.is_empty() { crate::i18n::tr("Go back") } else { then.as_str() })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(then, String::new(), crate::i18n::tr("Go back"));
                                            for n in &profile_names {
                                                ui.selectable_value(then, n.clone(), n);
                                            }
                                        })
                                        .response
                                        .on_hover_text(crate::i18n::tr("What happens when the button is let go: go back to the profile from before, or stay on another profile."));
                                });
                            }
                            Action::Gamepad { button } => {
                                fit(ui, 130.0);
                                egui::ComboBox::from_id_salt(("map-pad", i))
                                    .width(130.0)
                                    .selected_text(crate::i18n::tr(button.short_name()))
                                    .show_ui(ui, |ui| {
                                        for b in button_choices(false) {
                                            if b.is_touchpad_zone() {
                                                continue;
                                            }
                                            ui.selectable_value(button, b, crate::i18n::tr(b.short_name()));
                                        }
                                    });
                                if !virtual_on {
                                    ui.label(RichText::new(crate::i18n::tr("needs a virtual controller")).size(12.0).color(WARN));
                                }
                            }
                            Action::Xbox { button } => {
                                fit(ui, 130.0);
                                egui::ComboBox::from_id_salt(("map-xbox", i))
                                    .width(130.0)
                                    .selected_text(crate::i18n::tr(button.label()))
                                    .show_ui(ui, |ui| {
                                        for b in XboxPadButton::ALL {
                                            ui.selectable_value(button, b, crate::i18n::tr(b.label()));
                                        }
                                    });
                                if !xbox_on && can_hide {
                                    ui.label(RichText::new(crate::i18n::tr("hidden helper Xbox controller")).size(12.0).color(MUTED))
                                        .on_hover_text(crate::i18n::tr("This profile has no virtual Xbox controller, so DSZ keeps a helper one plugged in, hidden from games with HidHide. Only Windows (Task View, Game Bar) sees it. Needs usbip-win2."));
                                } else if !xbox_on {
                                    ui.label(RichText::new(crate::i18n::tr("games see a helper controller")).size(12.0).color(WARN))
                                        .on_hover_text(crate::i18n::tr("Without HidHide, DSZ plugs in a helper Xbox controller for the press, which games can see, and unplugs it 30 seconds after the last press. Windows needs about a second to see it, so the first press arrives late. Install HidHide on the Virtual controller page to hide it."));
                                }
                            }
                            Action::Run { path } => {
                                ui.add(egui::TextEdit::singleline(path).hint_text("C:\\path\\to\\app.exe").desired_width(220.0));
                                if button(ui, crate::i18n::tr("Browse…")).clicked() {
                                    if let Some(f) = rfd::FileDialog::new().pick_file() {
                                        *path = f.display().to_string();
                                    }
                                }
                            }
                            Action::TurnOffController => {
                                ui.label(RichText::new(crate::i18n::tr("powers off over Bluetooth")).size(12.0).color(MUTED))
                                    .on_hover_text(crate::i18n::tr("Drops the Bluetooth link, which turns the controller off. Plugged in over USB, it stays on."));
                            }
                            _ => {}
                        }
                    });
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(crate::i18n::tr("Together with")).size(12.5).color(MUTED));
                        let mut chosen: Vec<Button> = m.with.iter().copied().filter(|b| *b != m.button).collect();
                        let label = if chosen.is_empty() {
                            crate::i18n::tr("nothing (single button)").to_string()
                        } else {
                            chosen.iter().map(|b| crate::i18n::tr(b.short_name())).collect::<Vec<_>>().join(" + ")
                        };
                        fit(ui, 170.0);
                        egui::ComboBox::from_id_salt(("map-with", i)).width(170.0).selected_text(label).show_ui(ui, |ui| {
                            for b in button_choices(edge) {
                                if b == m.button {
                                    continue;
                                }
                                let mut on = chosen.contains(&b);
                                if ui.checkbox(&mut on, crate::i18n::tr(b.short_name())).changed() {
                                    if on {
                                        chosen.push(b);
                                    } else {
                                        chosen.retain(|x| *x != b);
                                    }
                                }
                            }
                        });
                        chosen.sort();
                        m.with = chosen;
                        ui.add_space(10.0);
                        if m.action != Action::AppSwitcherHold {
                        ui.checkbox(&mut m.mute, crate::i18n::tr("Hide from game"))
                            .on_hover_text(if m.is_combo() {
                                crate::i18n::tr("While the combo is held, the virtual controller doesn't see these buttons.")
                            } else {
                                crate::i18n::tr("The virtual controller never sees this button. Needs a virtual controller.")
                            });
                        }
                        if button(ui, crate::i18n::tr("Copy to profile…")).clicked() { copy_request = Some(m.clone()); }
                    });
                    let held = matches!(m.action, Action::HoldProfile { .. } | Action::AppSwitcherHold);
                    let navigation = matches!(m.action, Action::AppSwitcherNext | Action::AppSwitcherPrevious);
                    if held && m.action == Action::AppSwitcherHold {
                        m.gesture = Gesture::Press;
                        m.mode = HoldStyle::Hold;
                        muted(ui, crate::i18n::tr("Arms the switcher without pressing Alt. Next and Previous must use the same base button and all these extra buttons, plus at least one more. Configure each prefix button above to control game pass-through."));
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(crate::i18n::tr("Behavior")).size(12.5).color(MUTED));
                        if held {
                            // The profile lasts as long as the hold.
                            ui.label(RichText::new(crate::i18n::tr("While held")).size(12.5).color(TEXT));
                        } else if navigation {
                            segmented(ui, &mut m.mode, &[(HoldStyle::Tap, crate::i18n::tr("Tap")), (HoldStyle::Turbo, "Turbo")]);
                            if !matches!(m.mode, HoldStyle::Tap | HoldStyle::Turbo) { m.mode = HoldStyle::Tap; }
                        } else {
                            fit(ui, 290.0);
                            segmented(
                                ui,
                                &mut m.mode,
                                &[
                                    (HoldStyle::Hold, crate::i18n::tr("Hold")),
                                    (HoldStyle::Tap, crate::i18n::tr("Tap")),
                                    (HoldStyle::Turbo, "Turbo"),
                                    (HoldStyle::Toggle, crate::i18n::tr("Toggle")),
                                ],
                            );
                        }
                        if m.mode == HoldStyle::Turbo && !held {
                            fit(ui, 260.0);
                            ui.add(egui::Slider::new(&mut m.turbo_ms, 30..=1000).suffix(" ms").text(crate::i18n::tr("interval")));
                        }
                        match m.gesture {
                            Gesture::LongPress => {
                                fit(ui, 260.0);
                                ui.add(egui::Slider::new(&mut m.long_ms, 150..=2000).suffix(" ms").text(crate::i18n::tr("hold for")));
                            }
                            Gesture::DoubleTap => {
                                fit(ui, 260.0);
                                ui.add(egui::Slider::new(&mut m.double_ms, 120..=800).suffix(" ms").text("window"));
                            }
                            _ => {}
                        }
                    });
                    for (_, other, kind) in warnings.iter().filter(|w| w.0 == i) {
                        let message = match kind {
                            WarningKind::Duplicate => "Duplicate trigger: both mappings will run.",
                            WarningKind::Overlap => "Overlapping combos can run together when all their buttons are held.",
                            WarningKind::Solo => "A solo action can run before a combo forms. Modifier consumption delays solo actions.",
                            WarningKind::Gesture => "Press can fire before long press or double tap completes; gesture bindings do not cancel each other.",
                            WarningKind::Leakage => "Combo buttons can reach the game before the combo forms, including after a timed modifier's delay.",
                            WarningKind::NoSession => "App switching needs a Hold mapping with the same base button and a strict subset of these buttons.",
                            WarningKind::NoVirtual => "Hiding buttons and consuming modifiers requires virtual controller output.",
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(crate::i18n::tr(message)).color(WARN).size(12.0));
                            if let Some(j) = other { muted(ui, &crate::localized_format!("See mapping {}", "Veja o mapeamento {}", "Ver asignación {}", j + 1)); }
                        });
                    }
                });
            ui.add_space(8.0);
        }
        if let Some(i) = remove {
            p.mappings.remove(i);
            self.capture_key = None;
        }
        if let Some(c) = capture {
            self.capture_key = c;
        }
        if let Some(m) = copy_request {
            self.mapping_copy = Some((p.name.clone(), m, Vec::new(), CopyPolicy::Skip));
            ui.scroll_to_rect(egui::Rect::from_min_size(ui.min_rect().min, egui::vec2(1.0, 1.0)), Some(egui::Align::Min));
        }
        ui.horizontal(|ui| {
            if primary_button(ui, crate::i18n::tr("+ Add mapping")).clicked() {
                p.mappings.push(Mapping {
                    action: Action::Key {
                        vk: 0x20,
                        ctrl: false,
                        shift: false,
                        alt: false,
                        win: false,
                    },
                    ..Default::default()
                });
            }
            if !p.mappings.is_empty() && button(ui, crate::i18n::tr("Remove all")).clicked() {
                p.mappings.clear();
            }
        });
        ui.add_space(12.0);
        note(
            ui,
            crate::i18n::tr("Hold sends the output while held, Tap sends one click, Turbo repeats, Toggle latches on and off. Long press and double tap fire only when the gesture completes. Use profile while held switches while the button is down and goes back when it comes up; the virtual controller stays the same. Touchpad Left, Center, and Right follow the finger when the pad clicks: left of 640, the middle, and 1280 or further, on the 1920-wide pad. A click with no finger is Touchpad only. Hiding a zone from the game hides that click."),
            ACCENT,
        );
    }

    fn mapping_copy_editor(&mut self, ui: &mut egui::Ui, current: &mut Profile) {
        if let Some((source, mapping, mut targets, mut policy)) = self.mapping_copy.take() {
            let mut apply = false;
            let mut cancel = false;
            card(ui, crate::i18n::tr("Copy mapping"), &source, |ui| {
                let buttons = mapping.buttons().map(|b| crate::i18n::tr(b.short_name())).collect::<Vec<_>>().join(" + ");
                let action = match mapping.action {
                    Action::Media { key } => crate::i18n::tr(key.label()),
                    _ => crate::i18n::tr(KINDS.iter().find(|k| k.0 == akind(&mapping.action)).map(|k| k.1).unwrap_or("")),
                };
                ui.label(format!("{buttons} → {action}"));
                muted(ui, crate::i18n::tr("Select destination profiles. Copies the entire mapping; each profile keeps its own modifier policies."));
                for name in self.engine.profiles.names().into_iter().filter(|n| *n != source) {
                    let mut selected = targets.contains(&name);
                    let duplicates = self.engine.profiles.find(&name).map_or(0, |p| p.mappings.iter().filter(|m| mapping::same_trigger(m, &mapping)).count());
                    ui.horizontal_wrapped(|ui| {
                        if ui.checkbox(&mut selected, &name).changed() {
                            if selected { targets.push(name.clone()); } else { targets.retain(|n| *n != name); }
                        }
                        if duplicates != 0 { ui.label(RichText::new(crate::i18n::tr("Matching trigger exists")).color(WARN)); }
                    });
                }
                segmented(ui, &mut policy, &[(CopyPolicy::Skip, crate::i18n::tr("Skip conflicts")), (CopyPolicy::Replace, crate::i18n::tr("Replace conflicts")), (CopyPolicy::KeepBoth, crate::i18n::tr("Keep both"))]);
                ui.horizontal(|ui| {
                    apply = ui.add_enabled(!targets.is_empty(), egui::Button::new(crate::i18n::tr("Copy selected"))).clicked();
                    cancel = button(ui, crate::i18n::tr("Cancel")).clicked();
                });
            });
            if apply {
                self.capture_key = None;
                let mut undo = Vec::new();
                for name in targets {
                    if let Some(p) = self.engine.profiles.find(&name) {
                        let mut next = (*p).clone();
                        if mapping::copy_mapping(&mut next, &mapping, policy) && next != *p {
                            undo.push((name, p.mappings.clone(), next.mappings.clone()));
                            if current.name == next.name { *current = next.clone(); }
                            self.engine.profiles.update(next);
                        }
                    }
                }
                if undo.is_empty() { self.engine.toast(crate::i18n::tr("Nothing copied; matching triggers were skipped.")); }
                else { self.mapping_copy_undo = undo; self.engine.toast(crate::i18n::tr("Mapping copied. Undo is available on this page.")); }
            } else if !cancel { self.mapping_copy = Some((source, mapping, targets, policy)); }
            ui.add_space(10.0);
        }
        if !self.mapping_copy_undo.is_empty() && button(ui, crate::i18n::tr("Undo last mapping copy")).clicked() {
            self.capture_key = None;
            let mut skipped = false;
            for (name, before, after) in std::mem::take(&mut self.mapping_copy_undo) {
                if let Some(p) = self.engine.profiles.find(&name) {
                    let mut restored = (*p).clone();
                    if mapping::undo_copy(&mut restored, &before, &after) {
                        if current.name == restored.name { *current = restored.clone(); }
                        self.engine.profiles.update(restored);
                    } else { skipped = true; }
                }
            }
            if skipped { self.engine.warn_toast(crate::i18n::tr("Undo skipped profiles whose mappings changed after copying.")); }
        }
    }
}
