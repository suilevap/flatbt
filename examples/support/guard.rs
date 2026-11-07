//! Separate gameplay state from the guard's perception and reasoning.
//! Shared by the enum and dynamic-act examples.

/// Facts gathered from the world before a decision.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Observed {
    pub post: f32,
    pub intruder: f32,
    pub ammo: u32,
    pub magazine: u32,
}

/// The AI's view of the world and the alternatives it has considered.
#[derive(Default)]
pub struct Guard {
    pub observed: Observed,
    pub considered: Vec<&'static str>,
}

impl Guard {
    pub fn in_range(&self) -> bool {
        (self.observed.post - self.observed.intruder).abs() <= 1.0
    }

    pub fn dry(&self) -> bool {
        self.observed.ammo == 0
    }
}

/// Actual gameplay state. Only the driver receives this object.
pub struct World {
    pub post: f32,
    pub intruder: f32,
    pub ammo: u32,
    pub magazine: u32,
    /// Filled by the driver, so the trace shows what the world did.
    pub trace: Vec<String>,
}

impl World {
    pub fn new(ammo: u32) -> Self {
        Self {
            post: 0.0,
            intruder: 4.0,
            ammo,
            magazine: 3,
            trace: Vec::new(),
        }
    }

    pub fn observe(&self) -> Observed {
        Observed {
            post: self.post,
            intruder: self.intruder,
            ammo: self.ammo,
            magazine: self.magazine,
        }
    }

    // --- what the world does when it is told to -----------------------------
    //
    // Every number here belongs to the game: how far a step is, what a shot
    // costs, how fast a magazine fills. No tree in either example saw them.

    pub fn step_towards(&mut self, target: f32) {
        self.post += (target - self.post).clamp(-1.0, 1.0);
        self.trace.push(format!("walk to {}", self.post));
    }

    pub fn fire(&mut self) {
        self.ammo = self.ammo.saturating_sub(1);
        self.trace.push(format!("fire ({} left)", self.ammo));
    }

    pub fn load_one(&mut self) {
        self.ammo += 1;
        self.trace.push(format!("load ({})", self.ammo));
    }
}
