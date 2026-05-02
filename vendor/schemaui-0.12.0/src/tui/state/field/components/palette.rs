use std::borrow::Cow;

/// Numeric stepping behaviour shared by text/numeric components.
#[derive(Debug, Clone)]
pub struct NumericTuning {
    pub integer_step: i64,
    pub integer_fast_step: Option<i64>,
    pub float_step: f64,
    pub float_fast_step: Option<f64>,
}

impl NumericTuning {
    #[inline]
    pub fn step_i64(&self, fast: bool) -> i64 {
        if fast {
            self.integer_fast_step.unwrap_or(self.integer_step)
        } else {
            self.integer_step
        }
    }

    #[inline]
    pub fn step_f64(&self, fast: bool) -> f64 {
        if fast {
            self.float_fast_step.unwrap_or(self.float_step)
        } else {
            self.float_step
        }
    }

    pub fn with_integer_step(mut self, step: i64) -> Self {
        self.integer_step = step.max(1);
        self
    }

    pub fn with_integer_fast_step(mut self, step: i64) -> Self {
        self.integer_fast_step = Some(step.max(1));
        self
    }

    pub fn with_float_step(mut self, step: f64) -> Self {
        self.float_step = step.max(f64::EPSILON);
        self
    }

    pub fn with_float_fast_step(mut self, step: f64) -> Self {
        self.float_fast_step = Some(step.max(f64::EPSILON));
        self
    }
}

impl Default for NumericTuning {
    fn default() -> Self {
        Self {
            integer_step: 1,
            integer_fast_step: Some(10),
            float_step: 1.0,
            float_fast_step: Some(10.0),
        }
    }
}

/// Presentation & toggling configuration for boolean components.
#[derive(Debug, Clone)]
pub struct BoolTogglePresentation {
    pub true_label: Cow<'static, str>,
    pub false_label: Cow<'static, str>,
    pub toggle_with_space: bool,
    pub toggle_with_arrows: bool,
}

impl BoolTogglePresentation {
    pub fn with_labels(
        mut self,
        true_label: impl Into<Cow<'static, str>>,
        false_label: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.true_label = true_label.into();
        self.false_label = false_label.into();
        self
    }

    pub fn with_toggle_with_space(mut self, enabled: bool) -> Self {
        self.toggle_with_space = enabled;
        self
    }

    pub fn with_toggle_with_arrows(mut self, enabled: bool) -> Self {
        self.toggle_with_arrows = enabled;
        self
    }
}

impl Default for BoolTogglePresentation {
    fn default() -> Self {
        Self {
            true_label: Cow::Borrowed("true"),
            false_label: Cow::Borrowed("false"),
            toggle_with_space: true,
            toggle_with_arrows: true,
        }
    }
}

/// Behaviour toggles for enum-like components.
#[derive(Debug, Clone)]
pub struct EnumBehaviour {
    pub wrap_around: bool,
}

impl EnumBehaviour {
    pub fn with_wrap(mut self, wrap: bool) -> Self {
        self.wrap_around = wrap;
        self
    }
}

impl Default for EnumBehaviour {
    fn default() -> Self {
        Self { wrap_around: true }
    }
}

/// User-facing string hints for collection overlays.
#[derive(Debug, Clone)]
pub struct CollectionHints {
    pub overlay_instructions: Cow<'static, str>,
    pub list_hint: Cow<'static, str>,
}

impl CollectionHints {
    pub fn with_overlay_instructions(mut self, instructions: impl Into<Cow<'static, str>>) -> Self {
        self.overlay_instructions = instructions.into();
        self
    }

    pub fn with_list_hint(mut self, hint: impl Into<Cow<'static, str>>) -> Self {
        self.list_hint = hint.into();
        self
    }
}

impl Default for CollectionHints {
    fn default() -> Self {
        Self {
            overlay_instructions: Cow::Borrowed(
                "Ctrl+N add • Ctrl+D remove • Ctrl+←/→ select • Ctrl+↑/↓ reorder",
            ),
            list_hint: Cow::Borrowed("(Ctrl+Left/Right select, Ctrl+E edit)"),
        }
    }
}

/// Copy text for composite selector prompts.
#[derive(Debug, Clone)]
pub struct CompositeHints {
    pub single_variant_hint: Cow<'static, str>,
    pub multi_variant_hint: Cow<'static, str>,
}

impl CompositeHints {
    pub fn with_single_hint(mut self, hint: impl Into<Cow<'static, str>>) -> Self {
        self.single_variant_hint = hint.into();
        self
    }

    pub fn with_multi_hint(mut self, hint: impl Into<Cow<'static, str>>) -> Self {
        self.multi_variant_hint = hint.into();
        self
    }
}

impl Default for CompositeHints {
    fn default() -> Self {
        Self {
            single_variant_hint: Cow::Borrowed(" (Enter to choose)"),
            multi_variant_hint: Cow::Borrowed(" (Enter to toggle)"),
        }
    }
}

/// Aggregated component palette that mirrors shadcn-like primitives.
#[derive(Debug, Clone, Default)]
pub struct ComponentPalette {
    pub numeric: NumericTuning,
    pub bools: BoolTogglePresentation,
    pub enums: EnumBehaviour,
    pub collection: CollectionHints,
    pub composite: CompositeHints,
}

impl ComponentPalette {
    pub fn with_numeric(mut self, tuning: NumericTuning) -> Self {
        self.numeric = tuning;
        self
    }

    pub fn with_bool_presentation(mut self, presentation: BoolTogglePresentation) -> Self {
        self.bools = presentation;
        self
    }

    pub fn with_enum_behaviour(mut self, behaviour: EnumBehaviour) -> Self {
        self.enums = behaviour;
        self
    }

    pub fn with_collection_hints(mut self, hints: CollectionHints) -> Self {
        self.collection = hints;
        self
    }

    pub fn with_composite_hints(mut self, hints: CompositeHints) -> Self {
        self.composite = hints;
        self
    }
}
