//! Integration tests for AgentLoop wiring real domain crates
//! (agentj-session, agentj-model, agentj-tools, agentj-context).

use agentj_context::ContextPipeline;
use agentj_core::agent_loop::{AgentEvent, AgentLoop};
use agentj_core::module::*;
use agentj_core::types::*;
use agentj_model::mock::MockModelProvider;
use agentj_session::InMemoryStore;
use agentj_tools::builtins::ReadTool;
use std::sync::Arc;

struct MockConfig;
impl ConfigProvider for MockConfig {
    fn get_str(&self, _k: &str) -> Option<String> {
        None
    }
    fn get_int(&self, _k: &str) -> Option<i64> {
        None
    }
    fn providers(&self) -> Vec<(String, String)> {
        vec![]
    }
}

#[tokio::test]
async fn e2e_streaming_with_real_modules() {
    let tools = Arc::new(ToolRegistry::new());
    tools.register(Arc::new(ReadTool));
    let providers = Arc::new(ProviderRegistry::new());
    let session = Arc::new(InMemoryStore::new());
    let config: Arc<dyn ConfigProvider> = Arc::new(MockConfig);
    let mock = Arc::new(MockModelProvider::new(vec![
        Ok(ModelEvent::Delta("Response".into())),
        Ok(ModelEvent::Done),
    ]));
    providers.register("test-model".into(), mock);

    let ctx = KernelContext {
        session: session.clone(),
        tools,
        providers,
        config,
    };
    let agent_loop = AgentLoop::new(
        ctx,
        Arc::new(ContextPipeline::new()),
        "You are helpful.".into(),
    );
    let sid = agent_loop.create_session().unwrap();

    let mut rx = agent_loop.run_turn_stream(&sid, "hi");
    let mut text = String::new();
    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::Delta(d) => text.push_str(&d),
            AgentEvent::Done { .. } => break,
            AgentEvent::Error(e) => panic!("agent error: {e}"),
            _ => {}
        }
    }
    assert_eq!(text, "Response");
    assert_eq!(session.get(&sid).unwrap().len(), 2); // user + assistant
}
