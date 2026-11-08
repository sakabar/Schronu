use dioxus::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct CopyTaskId {
    pub(super) _handler: EventHandler<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CopyNotice {
    succeeded: bool,
}

impl CopyNotice {
    fn message(self) -> &'static str {
        if self.succeeded {
            "タスクIDをコピーしました"
        } else {
            "タスクIDをコピーできませんでした"
        }
    }

    #[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
    fn duration_ms(self) -> u32 {
        if self.succeeded {
            1_500
        } else {
            4_000
        }
    }
}

// Serialize the value as a JS string literal; task names never enter the script.
fn copy_script(task_id: &str) -> String {
    let task_id = serde_json::to_string(task_id).expect("string serialization is infallible");
    format!(
        "try {{ await navigator.clipboard.writeText({task_id}); return true; }} catch (_) {{ return false; }}"
    )
}

#[component]
pub(super) fn CopyTaskIdProvider(children: Element) -> Element {
    let mut notice = use_signal(|| None::<CopyNotice>);
    let mut generation = use_signal(|| 0_u64);
    let copy = use_callback(move |task_id: String| {
        let request = generation() + 1;
        generation.set(request);
        notice.set(None);
        // Start the browser call within the click handler to preserve user activation.
        let evaluation = document::eval(&copy_script(&task_id));
        spawn(async move {
            let succeeded = evaluation.join::<bool>().await.unwrap_or(false);
            if generation() != request {
                return;
            }
            let result = CopyNotice { succeeded };
            notice.set(Some(result));
            #[cfg(all(feature = "web", target_arch = "wasm32"))]
            {
                gloo_timers::future::TimeoutFuture::new(result.duration_ms()).await;
                if generation() == request {
                    notice.set(None);
                }
            }
        });
    });
    use_context_provider(|| CopyTaskId { _handler: copy });
    rsx! {
        {children}
        div { class: "task-copy-notice", role: "status", aria_live: "polite", aria_atomic: "true",
            if let Some(result) = notice() {
                span { "{result.message()}" }
            }
        }
    }
}

#[component]
pub(super) fn TaskIdCopyButton(task_id: String, name: String) -> Element {
    let copy = try_consume_context::<CopyTaskId>();
    rsx! {
        button {
            class: "task-id-copy",
            r#type: "button",
            title: "タスクIDをコピー",
            aria_label: format!("{name}: タスクIDをコピー"),
            onclick: move |_| {
                if let Some(copy) = copy {
                    copy._handler.call(task_id.clone());
                }
            },
            "{name}"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_reports_success_and_failure_with_distinct_durations() {
        let success = CopyNotice { succeeded: true };
        let failure = CopyNotice { succeeded: false };
        assert_eq!(success.message(), "タスクIDをコピーしました");
        assert_eq!(success.duration_ms(), 1_500);
        assert_eq!(failure.message(), "タスクIDをコピーできませんでした");
        assert_eq!(failure.duration_ms(), 4_000);
    }

    #[test]
    fn script_encodes_the_exact_clipboard_text() {
        let task_id = "id\"\\\n";
        let script = copy_script(task_id);
        let literal = script
            .split_once("writeText(")
            .unwrap()
            .1
            .split_once(");")
            .unwrap()
            .0;
        assert_eq!(serde_json::from_str::<String>(literal).unwrap(), task_id);
        assert!(script.contains("catch (_) { return false; }"));
    }
}

#[cfg(all(test, feature = "server"))]
mod interaction_tests {
    use super::super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
    use super::*;
    use dioxus::dioxus_core::internal::generational_box::Owner;
    use futures::{channel::oneshot, Future, FutureExt};
    use std::{
        cell::RefCell,
        collections::VecDeque,
        pin::Pin,
        rc::Rc,
        task::{Context, Poll},
    };

    type Reply = Result<serde_json::Value, document::EvalError>;
    struct PendingEvaluation(oneshot::Receiver<Reply>);

    impl document::Evaluator for PendingEvaluation {
        fn poll_join(&mut self, cx: &mut Context<'_>) -> Poll<Reply> {
            match Pin::new(&mut self.0).poll(cx) {
                Poll::Ready(Ok(reply)) => Poll::Ready(reply),
                Poll::Ready(Err(_)) => Poll::Ready(Err(document::EvalError::Unsupported)),
                Poll::Pending => Poll::Pending,
            }
        }
        fn poll_recv(&mut self, _: &mut Context<'_>) -> Poll<Reply> {
            Poll::Pending
        }
        fn send(&self, _: serde_json::Value) -> Result<(), document::EvalError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct ClipboardDocument {
        scripts: RefCell<Vec<String>>,
        replies: RefCell<VecDeque<oneshot::Receiver<Reply>>>,
        owner: Owner,
    }

    impl document::Document for ClipboardDocument {
        fn eval(&self, script: String) -> document::Eval {
            self.scripts.borrow_mut().push(script);
            document::Eval::new(
                self.owner.insert(Box::new(PendingEvaluation(
                    self.replies
                        .borrow_mut()
                        .pop_front()
                        .expect("clipboard reply"),
                ))),
            )
        }
    }

    #[derive(Clone)]
    struct HarnessProps {
        document: Rc<ClipboardDocument>,
    }
    fn harness(props: HarnessProps) -> Element {
        use_context_provider(|| props.document.clone() as Rc<dyn document::Document>);
        rsx! {
            CopyTaskIdProvider {
                TaskIdCopyButton { task_id: "00000000-0000-0000-0000-000000000001", name: "contract task" }
            }
        }
    }

    fn settle(dom: &mut VirtualDom) {
        let _ = dom.wait_for_work().now_or_never();
        dom.render_immediate_to_vec();
    }

    #[test]
    fn clicks_write_only_the_id_and_report_success_failure_and_latest_result() {
        let document = Rc::new(ClipboardDocument::default());
        let mut replies = Vec::new();
        for _ in 0..5 {
            let (sender, receiver) = oneshot::channel();
            replies.push(sender);
            document.replies.borrow_mut().push_back(receiver);
        }
        let mut dom = VirtualDom::new_with_props(
            harness,
            HarnessProps {
                document: document.clone(),
            },
        );
        let ids = rebuild_with_click_listeners(&mut dom);
        let id = ids[0];
        assert_eq!(ids.len(), 1);
        for result in [
            Ok(serde_json::json!(true)),
            Ok(serde_json::json!(false)),
            Err(document::EvalError::Unsupported),
        ] {
            dispatch_click(&dom, id);
            let success = result
                .as_ref()
                .is_ok_and(|value| value == &serde_json::json!(true));
            replies.remove(0).send(result).unwrap();
            settle(&mut dom);
            let html = dioxus::ssr::render(&dom);
            assert!(
                html.contains(CopyNotice { succeeded: success }.message()),
                "{html}"
            );
            assert!(
                !html.contains(
                    CopyNotice {
                        succeeded: !success
                    }
                    .message()
                ),
                "{html}"
            );
        }
        dispatch_click(&dom, id);
        dispatch_click(&dom, id);
        let stale = replies.remove(0);
        replies.remove(0).send(Ok(serde_json::json!(true))).unwrap();
        settle(&mut dom);
        stale.send(Ok(serde_json::json!(false))).unwrap();
        settle(&mut dom);
        let html = dioxus::ssr::render(&dom);
        assert!(html.contains("タスクIDをコピーしました"), "{html}");
        assert!(!html.contains("タスクIDをコピーできませんでした"), "{html}");
        let scripts = document.scripts.borrow();
        assert_eq!(scripts.len(), 5);
        for script in scripts.iter() {
            assert_eq!(script, &copy_script("00000000-0000-0000-0000-000000000001"));
        }
    }
}
