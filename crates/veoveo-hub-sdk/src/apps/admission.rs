//! Exact App tool admission over the hub-projected, caller-visible catalog.
use crate::identity::LocalToolName;
use rmcp::model::{Resource, Tool};

use crate::apps::{is_app_resource, tool_app_link};

/// Callers supply a resource and tools read through current MCP authority.
/// App-supplied metadata must never enter this projection.
pub fn resolve_app_tool<'a>(
    resource: &Resource,
    tools: &'a [Tool],
    alias: &str,
) -> Option<&'a Tool> {
    LocalToolName::parse(alias).ok()?;
    if alias.contains("__") {
        return None;
    }
    if !is_app_resource(resource) {
        return None;
    }
    let (owner, page) = resource.uri.strip_prefix("ui://")?.split_once('/')?;
    if owner.is_empty() || page.is_empty() || resource.uri.contains("..") {
        return None;
    }
    let target = format!("{owner}__{alias}");
    tools.iter().find(|tool| {
        tool.name == target
            && tool_app_link(tool)
                .is_some_and(|link| link.visible_to_app() && link.resource_uri == resource.uri)
    })
}

/// Reauthorize an already resolved intent just before dispatch and during recovery.
pub fn app_allows_tool(resource: &Resource, tools: &[Tool], target: &str) -> bool {
    if !is_app_resource(resource) {
        return false;
    }
    let Some((owner, _)) = resource
        .uri
        .strip_prefix("ui://")
        .and_then(|uri| uri.split_once('/'))
    else {
        return false;
    };
    let Some(tool) = tools.iter().find(|tool| tool.name == target) else {
        return false;
    };
    target.starts_with(&format!("{owner}__"))
        && tool_app_link(tool)
            .is_some_and(|link| link.visible_to_app() && link.resource_uri == resource.uri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::{UiVisibility, app_resource, link_tool_to_app};

    #[test]
    fn tools_require_the_exact_view_and_app_visibility() {
        let view = app_resource("ui://media/create.html", "Create");
        let other = app_resource("ui://media/other.html", "Other");
        let tool = link_tool_to_app(
            Tool::new("media__run", "Run", rmcp::model::JsonObject::new()),
            &view.uri,
            &[UiVisibility::App],
        );
        assert!(resolve_app_tool(&view, std::slice::from_ref(&tool), "run").is_some());
        assert!(app_allows_tool(
            &view,
            std::slice::from_ref(&tool),
            "media__run"
        ));
        assert!(resolve_app_tool(&view, std::slice::from_ref(&tool), "media__run").is_none());
        assert!(!app_allows_tool(
            &other,
            std::slice::from_ref(&tool),
            "media__run"
        ));
        assert!(!app_allows_tool(&view, &[], "media__run"));
    }

    #[test]
    fn a_view_reaches_no_tool_of_another_server() {
        let view = app_resource("ui://frames/edit.html", "Edit");
        let foreign = link_tool_to_app(
            Tool::new("time__now", "Time", rmcp::model::JsonObject::new()),
            &view.uri,
            &[UiVisibility::App],
        );
        let tools = [foreign];
        assert!(resolve_app_tool(&view, &tools, "now").is_none());
        assert!(!app_allows_tool(&view, &tools, "time__now"));
    }
}
