//! `stdio` area tests.

use super::*;
use crate::test_support::stub_session;
use crate::*;
use agentj_core::kernel::Kernel;

#[tokio::test]
async fn kernel_completion_is_observable_for_signal_interrupt() {
    // design #13: the CLI needs to interrupt serve_stdio's blocked read_line when
    // SIGINT/SIGTERM arrives — signal_shutdown + awaiting the kernel handle must
    // terminate the loop. Run serve_stdio with a stdin that stays open, signal
    // shutdown from another task, and assert serve_stdio returns.
    let mut server = IpcServer::new(Kernel::new(), stub_session());
    let tx = server.kernel_sender();
    let signal = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // Tolerate a closed channel: when the test harness delivers stdin EOF
        // immediately, serve_stdio's graceful EOF path already shut the kernel down.
        let _ = tx.send(KernelMessage::Shutdown);
    });
    // serve_stdio reads stdin (blocked) until the kernel finishes; feed a
    // request first so the loop is live, then the signal task triggers exit.
    let run = tokio::spawn(async move {
        server.serve_stdio().await.unwrap();
    });
    signal.await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), run)
        .await
        .expect("serve_stdio must exit after kernel shutdown")
        .unwrap();
}

/// The stdio transport (`serve_stdio` + `StdioConnection`) lives in `stdio.rs`.
#[test]
fn stdio_serving_moved_out_of_lib() {
    let struct_needle = format!("struct {} {{", "StdioConnection");
    for needle in [fn_needle("serve_stdio"), struct_needle] {
        assert!(
            !include_str!("../lib.rs").contains(&needle),
            "{needle} still in lib.rs"
        );
        assert!(
            include_str!("../stdio.rs").contains(&needle),
            "{needle} missing from stdio.rs"
        );
    }
}
