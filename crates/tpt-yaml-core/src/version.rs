/// The YAML version used to resolve implicit scalar tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum YamlVersion {
    Version11,
    Version12,
}

impl YamlVersion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Version11 => "1.1",
            Self::Version12 => "1.2",
        }
    }
}

impl core::str::FromStr for YamlVersion {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "1.1" | "1.1." => Ok(Self::Version11),
            "1.2" | "1.2." => Ok(Self::Version12),
            _ => Err(()),
        }
    }
}

impl core::fmt::Display for YamlVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}
