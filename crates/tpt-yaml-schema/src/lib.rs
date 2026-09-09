pub fn parse(_source: &str) -> Result<tpt_yaml_core::Document, tpt_yaml_core::YamlError> {
    Err(tpt_yaml_core::YamlError::new(
        0,
        tpt_yaml_core::ErrorKind::Other,
        "not implemented yet",
        _source,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder() {
        assert!(parse("ok: true").is_err());
    }
}
