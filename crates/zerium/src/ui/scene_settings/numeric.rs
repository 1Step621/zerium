use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum NumericSetting {
    Default,
    Min,
    Max,
}

impl NumericSetting {
    pub(super) const ALL: [Self; 3] = [Self::Default, Self::Min, Self::Max];

    pub(super) fn placeholder(self) -> String {
        match self {
            Self::Default => String::new(),
            Self::Min => t!("args.no_minimum").to_string(),
            Self::Max => t!("args.no_maximum").to_string(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct NumericSettingDraft {
    default: f64,
    min: Option<f64>,
    max: Option<f64>,
}

impl NumericSettingDraft {
    pub(super) fn for_schema(schema: &PropertySchema) -> Option<Self> {
        let constraints = schema.configuration_constraints(None);
        let ty = schema.scalar_type(None, None)?;
        let (min, max) = constraints.numeric_bounds(ty)?;
        Some(Self {
            default: schema.default_value().numeric_scalar()?,
            min: constraints.min.map(|_| min),
            max: constraints.max.map(|_| max),
        })
    }

    pub(super) fn parse(number: &NumericInput, text: &[SharedString; 3]) -> Option<Self> {
        let bound = |text: &str| {
            if text.trim().is_empty() {
                Some(None)
            } else {
                number.parse_number(text).map(Some)
            }
        };
        Some(Self {
            default: number.parse_number(&text[0])?,
            min: bound(&text[1])?,
            max: bound(&text[2])?,
        })
    }

    pub(super) fn value(self, setting: NumericSetting) -> f64 {
        match setting {
            NumericSetting::Default => self.default,
            NumericSetting::Min => self.min.unwrap_or(self.default),
            NumericSetting::Max => self.max.unwrap_or(self.default),
        }
    }

    pub(super) fn set(&mut self, setting: NumericSetting, value: f64) {
        match setting {
            NumericSetting::Default => self.default = value,
            NumericSetting::Min => self.min = Some(value),
            NumericSetting::Max => self.max = Some(value),
        }
    }

    pub(super) fn bounds(self, number: &NumericInput) -> (f64, f64) {
        let (lower, upper) = number.bounds();
        (self.min.unwrap_or(lower), self.max.unwrap_or(upper))
    }

    pub(super) fn normalize(
        mut self,
        number: &NumericInput,
        changed: NumericSetting,
    ) -> Option<Self> {
        // Convert adjustments at the input's precision before comparing dependent fields.
        let (lower, upper) = number.bounds();
        let constrain = |value: f64| {
            value.is_finite().then_some(())?;
            number
                .value_from_number(value.clamp(lower, upper))?
                .numeric_scalar()
        };
        self.default = constrain(self.default)?;
        let optional = |value| match value {
            Some(value) => constrain(value).map(Some),
            None => Some(None),
        };
        self.min = optional(self.min)?;
        self.max = optional(self.max)?;

        let (min, max) = self.bounds(number);
        if min > max {
            match changed {
                NumericSetting::Min => self.min = Some(max),
                NumericSetting::Max => self.max = Some(min),
                NumericSetting::Default => return None,
            }
        }
        let (min, max) = self.bounds(number);
        self.default = self.default.clamp(min, max);
        Some(self)
    }

    pub(super) fn to_domain(
        self,
        number: &NumericInput,
    ) -> Option<zerium_core::property::NumericSettings> {
        let default = number.value_from_number(self.default)?;
        let min = match self.min {
            Some(value) => Some(number.value_from_number(value)?),
            None => None,
        };
        let max = match self.max {
            Some(value) => Some(number.value_from_number(value)?),
            None => None,
        };
        zerium_core::property::NumericSettings::from_values(default, min, max)
    }

    pub(super) fn formatted(self, number: &NumericInput) -> [String; 3] {
        [
            number.format(self.default),
            self.min
                .map(|value| number.format(value))
                .unwrap_or_default(),
            self.max
                .map(|value| number.format(value))
                .unwrap_or_default(),
        ]
    }
}
