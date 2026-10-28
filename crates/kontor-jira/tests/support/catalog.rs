// Test-only legacy project data. No deployment compiles or falls back to it.
pub(crate) fn catalog() -> Result<kontor_jira::jira::SpecCatalog, kontor_jira::JiraError> {
    let mut catalog = kontor_jira::jira::SpecCatalog::empty();
    for text in [
        include_str!("../fixtures/ticket-fields-asma.json"),
        include_str!("../fixtures/ticket-fields-asma-epic.json"),
    ] {
        catalog.load_field_spec(text)?;
    }
    for text in [
        include_str!("../fixtures/external-workflow-asma.json"),
        include_str!("../fixtures/external-workflow-asma-high-stakes.json"),
        include_str!("../fixtures/external-workflow-asma-docs.json"),
        include_str!("../fixtures/external-workflow-asma-epic-v2.json"),
    ] {
        catalog.load_workflow_spec(text)?;
    }
    Ok(catalog)
}
