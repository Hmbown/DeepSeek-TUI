#[derive(Debug, Clone, Copy, Default)]
pub struct RootTabsStore {
    current: usize,
}

impl RootTabsStore {
    pub fn current(&self) -> usize {
        self.current
    }

    pub fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.current = 0;
        } else if self.current >= len {
            self.current = len.saturating_sub(1);
        }
    }

    pub fn set(&mut self, index: usize, len: usize) -> bool {
        let bounded = if len == 0 {
            0
        } else {
            index.min(len.saturating_sub(1))
        };
        if self.current == bounded {
            false
        } else {
            self.current = bounded;
            true
        }
    }
    pub fn advance(&mut self, delta: i32, len: usize) -> bool {
        if len == 0 {
            self.current = 0;
            return false;
        }
        let len_i32 = len as i32;
        let mut next = self.current as i32 + delta;
        next = ((next % len_i32) + len_i32) % len_i32;
        self.set(next as usize, len)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SectionTabsStore {
    current: usize,
}

impl SectionTabsStore {
    pub fn current(&self) -> usize {
        self.current
    }

    pub fn reset(&mut self) {
        self.current = 0;
    }

    pub fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.current = 0;
        } else if self.current >= len {
            self.current = len.saturating_sub(1);
        }
    }

    pub fn set(&mut self, index: usize, len: usize) -> bool {
        let bounded = if len == 0 {
            0
        } else {
            index.min(len.saturating_sub(1))
        };
        if self.current == bounded {
            false
        } else {
            self.current = bounded;
            true
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FieldListStore {
    current: usize,
}

impl FieldListStore {
    pub fn current(&self) -> usize {
        self.current
    }

    pub fn reset(&mut self) {
        self.current = 0;
    }

    pub fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.current = 0;
        } else if self.current >= len {
            self.current = len.saturating_sub(1);
        }
    }

    pub fn set(&mut self, index: usize, len: usize) -> bool {
        let bounded = if len == 0 {
            0
        } else {
            index.min(len.saturating_sub(1))
        };
        if self.current == bounded {
            false
        } else {
            self.current = bounded;
            true
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UiStores {
    pub root: RootTabsStore,
    pub sections: SectionTabsStore,
    pub fields: FieldListStore,
}

impl UiStores {
    pub fn new() -> Self {
        Self::default()
    }
}
