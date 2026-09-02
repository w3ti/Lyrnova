use std::path::Path;

use lyrnova_lib::app_server;

#[test]
#[ignore = "requires a locally installed Codex App Server"]
fn reads_local_account_without_exposing_credentials() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let status = app_server::read_account(workspace).expect("Codex App Server account/read");
    assert_eq!(status.backend, "codex_app_server");
    if let Some(account) = status.account {
        assert!(
            account
                .email
                .as_deref()
                .is_none_or(|email| !email.is_empty())
        );
        assert!(
            account
                .plan_type
                .as_deref()
                .is_none_or(|plan| !plan.is_empty())
        );
    }
}
