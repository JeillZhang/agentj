// agentj-model/benches/stream_bench.rs — criterion benchmark (compiles and runs)
use agentj_core::error::ModelError;
use agentj_core::module::ModelProvider;
use agentj_core::types::*;
use agentj_model::mock::MockModelProvider;
use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn bench_mock_stream(c: &mut Criterion) {
    c.bench_function("mock_stream_10_events", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
                let mut events: Vec<Result<ModelEvent, ModelError>> = (0..10)
                    .map(|i| Ok(ModelEvent::Delta(format!("event-{i}"))))
                    .collect();
                events.push(Ok(ModelEvent::Done));
                let provider = MockModelProvider::new(events);
                let stream = black_box(
                    provider
                        .chat_stream(vec![], vec![], ModelOptions::default())
                        .await
                        .unwrap(),
                );
                drop(stream);
            });
    });
}

criterion_group!(benches, bench_mock_stream);
criterion_main!(benches);
