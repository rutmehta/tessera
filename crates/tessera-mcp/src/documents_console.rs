//! Console entry points for layered documents and Actions.
use crate::Console;
use crate::actions::{self, ActionFile, ActionTarget, PlayReport, Recorder};
use crate::documents::{DocumentDescription, Documents};
use engine_api::id::DocumentId;
use engine_api::tools::{
    DocumentToolCall, DocumentToolOutput, DocumentToolRequest, DocumentToolResponse, ToolRequest,
    ToolResponse,
};
use engine_api::{EngineError, EngineResult};
use std::path::Path;

impl Console {
    /// Executes a layered-document tool call. Successful calls are
    /// recorded into the active Action recording.
    pub fn execute_document(&mut self, request: DocumentToolRequest) -> DocumentToolResponse {
        let result = self.documents.run(&request);
        if let (Ok(output), Some(recorder)) = (&result, &mut self.recorder) {
            recorder.record_document(&request, output);
        }
        result.into()
    }

    /// Opens a document file through `open_document` (recorded like any
    /// other call).
    pub fn open_document(&mut self, path: impl AsRef<Path>) -> EngineResult<DocumentId> {
        let path = path
            .as_ref()
            .to_str()
            .ok_or_else(|| EngineError::invalid("path", "UTF-8 path required"))?
            .to_owned();
        match self.execute_document(DocumentToolRequest {
            call: DocumentToolCall::OpenDocument { path },
            rationale: None,
            group: None,
            expect_head: None,
        }) {
            DocumentToolResponse::Ok(DocumentToolOutput::DocumentOpened { document, .. }) => {
                Ok(document)
            }
            DocumentToolResponse::Ok(_) => Err(EngineError::internal("unexpected output")),
            DocumentToolResponse::Error(e) => Err(e),
        }
    }

    /// Open documents.
    pub fn documents(&self) -> &Documents {
        &self.documents
    }

    /// Open documents, mutable (undo/redo, engines, closing).
    pub fn documents_mut(&mut self) -> &mut Documents {
        &mut self.documents
    }

    /// Composite preview at the pyramid level fitting `max_px`.
    pub fn render_document_preview(
        &mut self,
        document: DocumentId,
        max_px: u32,
    ) -> EngineResult<image::RgbaImage> {
        Ok(self.documents.render_preview(document, max_px)?.1)
    }

    /// `describe_document`.
    pub fn describe_document(
        &mut self,
        document: DocumentId,
        max_px: u32,
        thumbnail_px: Option<u32>,
    ) -> EngineResult<DocumentDescription> {
        self.documents.describe(document, max_px, thumbnail_px)
    }

    /// Starts recording executed calls as an Action.
    pub fn start_recording(&mut self, name: impl Into<String>) -> EngineResult<()> {
        if self.recorder.is_some() {
            return Err(EngineError::Conflict {
                message: "an action is already being recorded".into(),
            });
        }
        let name = name.into();
        if name.trim().is_empty() {
            return Err(EngineError::invalid("name", "must not be empty"));
        }
        self.recorder = Some(Recorder::new(name));
        Ok(())
    }

    /// True while recording.
    pub fn is_recording(&self) -> bool {
        self.recorder.is_some()
    }

    /// Stops recording and returns the action.
    pub fn stop_recording(&mut self) -> EngineResult<ActionFile> {
        self.recorder
            .take()
            .map(Recorder::finish)
            .ok_or_else(|| EngineError::invalid("actions", "not recording"))
    }

    /// Plays an action on `documents` (one per `$input`).
    pub fn play_action(
        &mut self,
        action: &ActionFile,
        documents: &[DocumentId],
    ) -> EngineResult<PlayReport> {
        actions::play(self, action, documents)
    }
}

impl ActionTarget for Console {
    fn run_document(&mut self, request: DocumentToolRequest) -> DocumentToolResponse {
        self.execute_document(request)
    }
    fn run_tool(&mut self, request: ToolRequest) -> ToolResponse {
        self.execute(request)
    }
}
