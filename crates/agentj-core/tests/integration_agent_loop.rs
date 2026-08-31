//! Integration test via bus-driven Kernel modules.

use agentj_context::ContextPipeline;
use agentj_core::agent_loop_module::AgentLoopModule;
use agentj_core::kernel::Kernel;
use agentj_core::module::{
    ConfigProvider, KernelContext, KernelMessage, ModuleRegister, ProviderRegistry, SessionStore,
    ToolRegistry,
};
use agentj_core::tool_executor::ToolExecutorModule;
use agentj_core::turn_runner::TurnRunner;
use agentj_core::types::{AgentEvent, ModelEvent};
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
async fn e2e_streaming_with_real_modules_via_bus() {
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
        tools: tools.clone(),
        providers: providers.clone(),
        config,
    };

    let mut kernel = Kernel::new();
    let mut register = ModuleRegister::with_services(
        Arc::clone(&tools),
        Arc::clone(&providers),
        kernel.services(),
    );

    let pipeline = Arc::new(ContextPipeline::new());
    let runner = TurnRunner::new(
        ctx.clone(),
        pipeline,
        "You are helpful.".into(),
        kernel.sender(),
        "tool_executor".into(),
    );

    kernel
        .register_module(
            Box::new(ToolExecutorModule::new(ctx.clone())),
            &ctx,
            &mut register,
        )
        .unwrap();
    kernel
        .register_module(Box::new(AgentLoopModule::new(runner)), &ctx, &mut register)
        .unwrap();

    let handle = kernel.start();
    let sid = session.create().unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    kernel
        .sender()
        .send(KernelMessage::UserRequest {
            session_id: sid.clone(),
            content: "hi".into(),
            attachments: vec![],
            reply: tx,
        })
        .unwrap();

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
    assert_eq!(session.get(&sid).unwrap().len(), 2);

    kernel.sender().send(KernelMessage::Shutdown).unwrap();
    handle.await.unwrap().unwrap();
}
