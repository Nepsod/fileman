pub(crate) fn log_entity_update<R>(
    context: &'static str,
    result: anyhow::Result<R>,
) -> Option<R> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            log::warn!("fileman foreground update failed ({context}): {error:#}");
            None
        }
    }
}
