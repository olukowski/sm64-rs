//! Raw controller values and button edges, without normalization or deadzones.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerInput {
    pub buttons: u16,
    pub stick_x: i8,
    pub stick_y: i8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickInput {
    pub raw: ControllerInput,
    pub held: u16,
    pub pressed: u16,
    pub released: u16,
}

/// Tracks all 16 button bits, including unassigned bits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonTracker {
    previous: ControllerInput,
}

impl ButtonTracker {
    pub fn new(previous: ControllerInput) -> Self {
        Self { previous }
    }

    pub fn previous(&self) -> ControllerInput {
        self.previous
    }

    /// Computes edges without advancing the previous input.
    pub fn derive(&self, raw: ControllerInput) -> TickInput {
        TickInput {
            raw,
            held: raw.buttons,
            pressed: raw.buttons & !self.previous.buttons,
            released: self.previous.buttons & !raw.buttons,
        }
    }

    pub fn update(&mut self, raw: ControllerInput) -> TickInput {
        let input = self.derive(raw);
        self.previous = raw;
        input
    }
}
