//! The world the two act examples share, so the only difference between them
//! is how the act is represented.

/// What the guard knows. The tree reads this and never writes it.
pub struct Guard {
    pub post: f32,
    pub intruder: f32,
    pub ammo: u32,
    pub magazine: u32,
    /// Filled by the driver, so the trace shows what the world did.
    pub trace: Vec<String>,
}

impl Guard {
    pub fn new(ammo: u32) -> Self {
        Self {
            post: 0.0,
            intruder: 4.0,
            ammo,
            magazine: 3,
            trace: Vec::new(),
        }
    }

    pub fn in_range(&self) -> bool {
        (self.post - self.intruder).abs() <= 1.0
    }

    pub fn dry(&self) -> bool {
        self.ammo == 0
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
