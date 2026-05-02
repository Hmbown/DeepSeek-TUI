use std::collections::HashMap;
use std::sync::Arc;

use crossterm::event::KeyEvent;

use crate::tui::state::FormCommand;

use super::keymap::KeymapStore;

#[derive(Debug, Clone, Copy)]
pub enum KeyAction {
    Save,
    Quit,
    ResetStatus,
    TogglePopup,
    EditComposite,
    ShowHelp,
    HelpClose,
    HelpPageStep(i32),
    HelpShortcutScroll(i32),
    HelpShortcutPage(i32),
    HelpShortcutHome,
    HelpShortcutEnd,
    HelpErrorScroll(i32),
    FieldStep(i32),
    SectionStep(i32),
    RootStep(i32),
    ListAddEntry,
    ListRemoveEntry,
    ListMove(i32),
    ListSelect(i32),
    Input(KeyEvent),
    None,
}

#[derive(Debug, Clone, Copy)]
pub enum AppCommand {
    Save,
    Quit,
    ResetStatus,
    TogglePopup,
    EditComposite,
    ShowHelp,
    ListAddEntry,
    ListRemoveEntry,
    ListMove(i32),
    ListSelect(i32),
}

#[derive(Debug, Clone)]
pub enum CommandDispatch {
    Form(FormCommand),
    App(AppCommand),
    Input(KeyEvent),
    None,
}

#[derive(Debug, Clone)]
pub struct KeyBindingMap {
    bindings: HashMap<KeyActionDiscriminant, CommandDispatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum KeyActionDiscriminant {
    Save,
    Quit,
    ResetStatus,
    TogglePopup,
    EditComposite,
    ShowHelp,
    FieldStepPos,
    FieldStepNeg,
    SectionStepPos,
    SectionStepNeg,
    RootStepPos,
    RootStepNeg,
    ListAdd,
    ListRemove,
    ListMoveUp,
    ListMoveDown,
    ListSelectLeft,
    ListSelectRight,
}

impl KeyBindingMap {
    pub fn builtin() -> Self {
        use CommandDispatch::{App, Form};
        use FormCommand::*;
        let mut bindings = HashMap::new();
        bindings.insert(KeyActionDiscriminant::Save, App(AppCommand::Save));
        bindings.insert(KeyActionDiscriminant::Quit, App(AppCommand::Quit));
        bindings.insert(KeyActionDiscriminant::FieldStepPos, Form(FocusNextField));
        bindings.insert(KeyActionDiscriminant::FieldStepNeg, Form(FocusPrevField));
        bindings.insert(
            KeyActionDiscriminant::SectionStepPos,
            Form(FormCommand::FocusNextSection(1)),
        );
        bindings.insert(
            KeyActionDiscriminant::SectionStepNeg,
            Form(FormCommand::FocusNextSection(-1)),
        );
        bindings.insert(
            KeyActionDiscriminant::RootStepPos,
            Form(FormCommand::FocusNextRoot(1)),
        );
        bindings.insert(
            KeyActionDiscriminant::RootStepNeg,
            Form(FormCommand::FocusNextRoot(-1)),
        );
        bindings.insert(
            KeyActionDiscriminant::TogglePopup,
            App(AppCommand::TogglePopup),
        );
        bindings.insert(
            KeyActionDiscriminant::EditComposite,
            App(AppCommand::EditComposite),
        );
        bindings.insert(KeyActionDiscriminant::ShowHelp, App(AppCommand::ShowHelp));
        bindings.insert(
            KeyActionDiscriminant::ResetStatus,
            App(AppCommand::ResetStatus),
        );
        bindings.insert(
            KeyActionDiscriminant::ListAdd,
            App(AppCommand::ListAddEntry),
        );
        bindings.insert(
            KeyActionDiscriminant::ListRemove,
            App(AppCommand::ListRemoveEntry),
        );
        bindings.insert(
            KeyActionDiscriminant::ListMoveUp,
            App(AppCommand::ListMove(-1)),
        );
        bindings.insert(
            KeyActionDiscriminant::ListMoveDown,
            App(AppCommand::ListMove(1)),
        );
        bindings.insert(
            KeyActionDiscriminant::ListSelectLeft,
            App(AppCommand::ListSelect(-1)),
        );
        bindings.insert(
            KeyActionDiscriminant::ListSelectRight,
            App(AppCommand::ListSelect(1)),
        );
        Self { bindings }
    }

    pub fn resolve(&self, action: KeyAction) -> CommandDispatch {
        match action {
            KeyAction::Save => self
                .bindings
                .get(&KeyActionDiscriminant::Save)
                .cloned()
                .unwrap_or(CommandDispatch::App(AppCommand::Save)),
            KeyAction::Quit => self
                .bindings
                .get(&KeyActionDiscriminant::Quit)
                .cloned()
                .unwrap_or(CommandDispatch::App(AppCommand::Quit)),
            KeyAction::ResetStatus => self
                .bindings
                .get(&KeyActionDiscriminant::ResetStatus)
                .cloned()
                .unwrap_or(CommandDispatch::App(AppCommand::ResetStatus)),
            KeyAction::ShowHelp => self
                .bindings
                .get(&KeyActionDiscriminant::ShowHelp)
                .cloned()
                .unwrap_or(CommandDispatch::App(AppCommand::ShowHelp)),
            KeyAction::TogglePopup => self
                .bindings
                .get(&KeyActionDiscriminant::TogglePopup)
                .cloned()
                .unwrap_or(CommandDispatch::App(AppCommand::TogglePopup)),
            KeyAction::EditComposite => self
                .bindings
                .get(&KeyActionDiscriminant::EditComposite)
                .cloned()
                .unwrap_or(CommandDispatch::App(AppCommand::EditComposite)),
            KeyAction::FieldStep(delta) => {
                let key = if delta >= 0 {
                    KeyActionDiscriminant::FieldStepPos
                } else {
                    KeyActionDiscriminant::FieldStepNeg
                };
                self.bindings.get(&key).cloned().unwrap_or({
                    if delta >= 0 {
                        CommandDispatch::Form(FormCommand::FocusNextField)
                    } else {
                        CommandDispatch::Form(FormCommand::FocusPrevField)
                    }
                })
            }
            KeyAction::SectionStep(delta) => {
                let key = if delta >= 0 {
                    KeyActionDiscriminant::SectionStepPos
                } else {
                    KeyActionDiscriminant::SectionStepNeg
                };
                self.bindings
                    .get(&key)
                    .cloned()
                    .unwrap_or(CommandDispatch::Form(FormCommand::FocusNextSection(delta)))
            }
            KeyAction::RootStep(delta) => {
                let key = if delta >= 0 {
                    KeyActionDiscriminant::RootStepPos
                } else {
                    KeyActionDiscriminant::RootStepNeg
                };
                self.bindings
                    .get(&key)
                    .cloned()
                    .unwrap_or(CommandDispatch::Form(FormCommand::FocusNextRoot(delta)))
            }
            KeyAction::ListAddEntry => self
                .bindings
                .get(&KeyActionDiscriminant::ListAdd)
                .cloned()
                .unwrap_or(CommandDispatch::App(AppCommand::ListAddEntry)),
            KeyAction::ListRemoveEntry => self
                .bindings
                .get(&KeyActionDiscriminant::ListRemove)
                .cloned()
                .unwrap_or(CommandDispatch::App(AppCommand::ListRemoveEntry)),
            KeyAction::ListMove(delta) => {
                let key = if delta >= 0 {
                    KeyActionDiscriminant::ListMoveDown
                } else {
                    KeyActionDiscriminant::ListMoveUp
                };
                self.bindings
                    .get(&key)
                    .cloned()
                    .unwrap_or(CommandDispatch::App(AppCommand::ListMove(delta)))
            }
            KeyAction::ListSelect(delta) => {
                let key = if delta >= 0 {
                    KeyActionDiscriminant::ListSelectRight
                } else {
                    KeyActionDiscriminant::ListSelectLeft
                };
                self.bindings
                    .get(&key)
                    .cloned()
                    .unwrap_or(CommandDispatch::App(AppCommand::ListSelect(delta)))
            }
            KeyAction::HelpClose
            | KeyAction::HelpPageStep(_)
            | KeyAction::HelpShortcutScroll(_)
            | KeyAction::HelpShortcutPage(_)
            | KeyAction::HelpShortcutHome
            | KeyAction::HelpShortcutEnd
            | KeyAction::HelpErrorScroll(_) => CommandDispatch::None,
            KeyAction::Input(event) => CommandDispatch::Input(event),
            KeyAction::None => CommandDispatch::None,
        }
    }
}

impl Default for KeyBindingMap {
    fn default() -> Self {
        Self::builtin()
    }
}

pub struct InputRouter {
    store: Arc<KeymapStore>,
}

impl InputRouter {
    pub(crate) fn new(store: Arc<KeymapStore>) -> Self {
        Self { store }
    }

    pub fn classify(&self, key: &KeyEvent) -> KeyAction {
        #[cfg(feature = "debug")]
        println!("{key:?}");
        self.store.classify(key).unwrap_or(KeyAction::Input(*key))
    }

    pub(crate) fn classify_for_contexts(
        &self,
        key: &KeyEvent,
        contexts: &[super::keymap::KeymapContext],
    ) -> KeyAction {
        #[cfg(feature = "debug")]
        println!("{key:?} @ {contexts:?}");
        self.store
            .classify_for_contexts(key, contexts)
            .unwrap_or(KeyAction::Input(*key))
    }
}
