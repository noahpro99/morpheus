//! The web app's side of the test bridge (`web-e2e` feature only): the page
//! cannot listen on a port, so it asks the test relay (crates/e2e/src/web.rs)
//! for the next script, runs it, and posts back what it returned, in the
//! desktop bridge's request and answer shapes. Only on a page opened with
//! `?e2e=1`. Never enable the feature in release builds.

use dioxus::prelude::*;

const BRIDGE_JS: &str = r#"
if (new URLSearchParams(location.search).has('e2e') && !window.__morpheusBridge) {
  window.__morpheusBridge = true;
  (async () => {
    const AsyncFunction = (async () => {}).constructor;
    for (;;) {
      let request;
      try {
        const r = await fetch('/__e2e/next', { cache: 'no-store' });
        if (r.status !== 200) continue;
        request = await r.json();
      } catch (_) {
        await new Promise(done => setTimeout(done, 200));
        continue;
      }
      let answer;
      try {
        const value = await new AsyncFunction(request.js)();
        answer = { id: request.id, ok: true, value: value === undefined ? null : value };
      } catch (err) {
        answer = { id: request.id, ok: false, error: 'Error running JS: ' + (err && err.stack ? String(err) : String(err)) };
      }
      let body;
      try { body = JSON.stringify(answer); }
      catch (err) { body = JSON.stringify({ id: request.id, ok: false, error: 'unserialisable result: ' + err }); }
      try { await fetch('/__e2e/answer', { method: 'POST', body }); } catch (_) {}
    }
  })();
}
"#;

/// Renders nothing; starts the page's bridge once.
#[component]
pub fn E2eBridge() -> Element {
    use_hook(|| {
        document::eval(BRIDGE_JS);
    });
    rsx! {}
}
