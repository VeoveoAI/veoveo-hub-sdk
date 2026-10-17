//! The authenticated read path for `{scheme}://docs` and its members.
use super::{
    DocumentEntry, DocumentId, DocumentPage, DocumentPageError, ServerDocs, docs_member_uri,
    page_documents,
};
use rmcp::{
    ErrorData, RoleServer,
    model::{
        ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, ResourceContents,
    },
    service::RequestContext,
};
use veoveo_types::{ResourceScheme, ResourceUriParts};

impl ServerDocs {
    pub fn document_page(
        &self,
        scheme: &ResourceScheme,
        cursor: Option<&DocumentId>,
    ) -> Result<DocumentPage, DocumentPageError> {
        let entries = self
            .iter()
            .map(|doc| {
                let id = DocumentId::parse(doc.id).map_err(|_| DocumentPageError::Duplicate)?;
                Ok(DocumentEntry {
                    uri: docs_member_uri(scheme, &id),
                    id,
                    title: doc.title.to_owned(),
                })
            })
            .collect::<Result<Vec<_>, DocumentPageError>>()?;
        page_documents(entries, cursor)
    }

    /// Handles only this server's docs routes. The Hub authentication middleware
    /// must have admitted the request; a missing forwarded identity fails closed. Domain
    /// resource handlers own every other route and policy.
    pub fn read_docs(
        &self,
        scheme: &ResourceScheme,
        request: &ReadResourceRequestParams,
        context: &RequestContext<RoleServer>,
    ) -> Result<Option<ReadResourceResponse>, ErrorData> {
        let parts = ResourceUriParts::parse(&request.uri)
            .map_err(|_| ErrorData::invalid_params("invalid resource URI", None))?;
        if parts.scheme() != scheme.as_str() || parts.authority() != "docs" {
            return Ok(None);
        }
        let http = context
            .extensions
            .get::<axum::http::request::Parts>()
            .ok_or_else(|| ErrorData::invalid_request(crate::HUB_ROUTING_REQUIRED, None))?;
        http.extensions
            .get::<crate::HubInternalIdentity>()
            .ok_or_else(|| ErrorData::invalid_request(crate::HUB_ROUTING_REQUIRED, None))?;
        self.read_authorized_docs(scheme, request)
    }

    /// For adapters with their own authenticated profile boundary. Call only after that
    /// boundary admits the caller.
    pub fn read_authorized_docs(
        &self,
        scheme: &ResourceScheme,
        request: &ReadResourceRequestParams,
    ) -> Result<Option<ReadResourceResponse>, ErrorData> {
        let parts = ResourceUriParts::parse(&request.uri)
            .map_err(|_| ErrorData::invalid_params("invalid resource URI", None))?;
        if parts.scheme() != scheme.as_str() || parts.authority() != "docs" {
            return Ok(None);
        }
        if request.request_state.is_some() || request.input_responses.is_some() {
            return Err(ErrorData::invalid_params(
                "document reads do not accept continuation state",
                None,
            ));
        }
        let path: Vec<_> = parts.path_segments().collect();
        let invalid = || ErrorData::invalid_params("invalid document address or cursor", None);
        let result = match path.as_slice() {
            [] => {
                if parts.query_parameters().keys().any(|k| k != "cursor") {
                    return Err(invalid());
                }
                let cursor = parts
                    .query_parameters()
                    .get("cursor")
                    .map(|s| DocumentId::parse(s.clone()))
                    .transpose()
                    .map_err(|_| invalid())?;
                let page = self
                    .document_page(scheme, cursor.as_ref())
                    .map_err(|_| invalid())?;
                ReadResourceResult::new(vec![
                    ResourceContents::text(
                        serde_json::to_string(&page).map_err(|_| {
                            ErrorData::internal_error("document page serialization failed", None)
                        })?,
                        &request.uri,
                    )
                    .with_mime_type("application/json"),
                ])
            }
            [id] if !parts.has_query() => {
                let id = DocumentId::parse(id.as_ref()).map_err(|_| invalid())?;
                let doc = self
                    .doc(id.as_str())
                    .ok_or_else(|| ErrorData::resource_not_found("unknown document", None))?;
                ReadResourceResult::new(vec![
                    ResourceContents::text(doc.body.to_owned(), &request.uri)
                        .with_mime_type("text/markdown"),
                ])
            }
            _ => return Err(invalid()),
        };
        Ok(Some(crate::private_resource_response(result, true)))
    }
}
