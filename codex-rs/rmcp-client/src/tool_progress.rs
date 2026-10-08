//! Call-scoped MCP progress routing. It never delays or changes tool execution.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::Weak;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use rmcp::model::ClientRequest;
use rmcp::model::GetMeta;
use rmcp::model::JsonRpcMessage;
use rmcp::model::NumberOrString;
use rmcp::model::ProgressNotificationParam;
use rmcp::model::ProgressToken;
use rmcp::model::ServerNotification;
use rmcp::service::RoleClient;
use rmcp::service::RxJsonRpcMessage;
use rmcp::service::TxJsonRpcMessage;
use rmcp::transport::IntoTransport;
use rmcp::transport::Transport;
use tokio::sync::mpsc;

// Consumed before serialization; the server only sees RMCP's original progress token.
pub(crate) const PROGRESS_ROUTE_META_KEY: &str = "overmind/internalProgressRoute";
type Aliases = Mutex<HashMap<ProgressToken, String>>;

#[derive(Clone, Debug, PartialEq)]
pub struct ToolProgress {
    pub progress: f64,
    pub total: Option<f64>,
    pub message: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct ProgressRegistry {
    routes: Arc<Mutex<HashMap<String, ProgressRoute>>>,
    next: Arc<AtomicU64>,
}

struct ProgressRoute {
    sender: mpsc::Sender<ToolProgress>,
    aliases: Vec<(Weak<Aliases>, ProgressToken)>,
}

pub(crate) struct ProgressRegistration {
    registry: ProgressRegistry,
    pub(crate) token: String,
}

impl ProgressRegistry {
    pub(crate) fn register(&self, sender: mpsc::Sender<ToolProgress>) -> ProgressRegistration {
        let token = format!(
            "overmind-progress-{}",
            self.next.fetch_add(1, Ordering::Relaxed)
        );
        self.routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                token.clone(),
                ProgressRoute {
                    sender,
                    aliases: Vec::new(),
                },
            );
        ProgressRegistration {
            registry: self.clone(),
            token,
        }
    }

    fn bind(&self, aliases: &Arc<Aliases>, token: ProgressToken, route: String) {
        let mut routes = self.routes.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(registered) = routes.get_mut(&route) {
            aliases
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(token.clone(), route);
            registered.aliases.push((Arc::downgrade(aliases), token));
        }
    }

    pub(crate) fn route(&self, params: ProgressNotificationParam) {
        let Ok(serde_json::Value::String(token)) = serde_json::to_value(&params.progress_token)
        else {
            return;
        };
        if !params.progress.is_finite() || params.progress < 0.0 {
            return;
        }
        let sender = self
            .routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&token)
            .map(|route| route.sender.clone());
        if let Some(sender) = sender {
            let _ = sender.try_send(ToolProgress {
                progress: params.progress,
                total: params
                    .total
                    .filter(|total| total.is_finite() && *total > 0.0),
                message: params
                    .message
                    .map(|message| message.chars().take(4_096).collect()),
            });
        }
    }
}

impl Drop for ProgressRegistration {
    fn drop(&mut self) {
        let route = self
            .registry
            .routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.token);
        if let Some(route) = route {
            for (aliases, token) in route.aliases {
                if let Some(aliases) = aliases.upgrade() {
                    let mut aliases = aliases.lock().unwrap_or_else(PoisonError::into_inner);
                    if aliases.get(&token) == Some(&self.token) {
                        aliases.remove(&token);
                    }
                }
            }
        }
    }
}

/// Associate RMCP's SDK-generated tokens with registered calls on this connection.
/// The SDK's token and request metadata stay intact on the wire and its timeout path.
pub(crate) fn capture_tool_progress<T, E, A>(
    transport: T,
    registry: ProgressRegistry,
) -> impl Transport<RoleClient, Error = E> + 'static
where
    T: IntoTransport<RoleClient, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    ProgressTransport {
        inner: transport.into_transport(),
        aliases: Arc::default(),
        registry,
    }
}

struct ProgressTransport<T> {
    inner: T,
    aliases: Arc<Aliases>,
    registry: ProgressRegistry,
}

impl<T: Transport<RoleClient> + 'static> Transport<RoleClient> for ProgressTransport<T> {
    type Error = T::Error;

    fn send(
        &mut self,
        mut message: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        if let JsonRpcMessage::Request(envelope) = &mut message
            && matches!(&envelope.request, ClientRequest::CallToolRequest(_))
        {
            let extension_route = envelope
                .request
                .get_meta_mut()
                .remove(PROGRESS_ROUTE_META_KEY);
            let params_route = match &mut envelope.request {
                ClientRequest::CallToolRequest(request) => request
                    .params
                    .meta
                    .as_mut()
                    .and_then(|meta| meta.remove(PROGRESS_ROUTE_META_KEY)),
                _ => None,
            };
            if let Some(route) = extension_route
                .or(params_route)
                .and_then(|value| value.as_str().map(str::to_owned))
                && let Some(token) = envelope.request.get_meta().get_progress_token()
            {
                self.registry.bind(&self.aliases, token, route);
            }
        }
        self.inner.send(message)
    }

    fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleClient>>> + Send {
        async move {
            let message = self.inner.receive().await?;
            if let JsonRpcMessage::Notification(envelope) = &message {
                let params = match &envelope.notification {
                    ServerNotification::ProgressNotification(notification) => {
                        Some(notification.params.clone())
                    }
                    // The pinned SDK can classify fractional JSON numbers as custom notifications.
                    ServerNotification::CustomNotification(notification)
                        if notification.method == "notifications/progress" =>
                    {
                        notification
                            .params
                            .clone()
                            .and_then(|params| serde_json::from_value(params).ok())
                    }
                    _ => None,
                };
                if let Some(mut params) = params {
                    let route = self
                        .aliases
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .get(&params.progress_token)
                        .cloned();
                    if let Some(route) = route {
                        params.progress_token = ProgressToken(NumberOrString::String(route.into()));
                        self.registry.route(params);
                    }
                }
            }
            Some(message)
        }
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InProcessTransportFactory;
    use crate::RmcpClient;
    use crate::elicitation_client_service::ElicitationClientService;
    use crate::rmcp_client::ElicitationPauseState;
    use futures::future::BoxFuture;
    use rmcp::RoleServer;
    use rmcp::ServerHandler;
    use rmcp::ServiceExt;
    use rmcp::model::CallToolRequestParams;
    use rmcp::model::CallToolResponse;
    use rmcp::model::CallToolResult;
    use rmcp::model::ClientInfo;
    use rmcp::model::ProtocolVersion;
    use rmcp::model::ServerJsonRpcMessage;
    use rmcp::service::serve_directly;
    use rmcp::transport::IntoTransport;
    use rmcp::transport::Transport;
    use std::time::Duration;
    use tokio::time::timeout;

    fn notification(token: &str, progress: f64, total: Option<f64>) -> ProgressNotificationParam {
        serde_json::from_value(serde_json::json!({ "progressToken": token, "progress": progress, "total": total, "message": "rendering" })).unwrap()
    }

    #[derive(Clone)]
    struct ProgressServer {
        observed_meta: mpsc::UnboundedSender<serde_json::Value>,
        release: Arc<tokio::sync::Notify>,
    }

    impl ServerHandler for ProgressServer {
        async fn call_tool(
            &self,
            _request: CallToolRequestParams,
            context: rmcp::service::RequestContext<RoleServer>,
        ) -> Result<CallToolResponse, rmcp::ErrorData> {
            let token = context
                .meta
                .get_progress_token()
                .expect("request progress token");
            let mut params = ProgressNotificationParam::new(token, 0.5);
            params.total = Some(1.5);
            params.message = Some("rendering".into());
            context
                .peer
                .notify_progress(params)
                .await
                .map_err(|error| rmcp::ErrorData::internal_error(error.to_string(), None))?;
            self.observed_meta
                .send(serde_json::to_value(&context.meta).unwrap())
                .unwrap();
            self.release.notified().await;
            Ok(CallToolResult::success(Vec::new()).into())
        }
    }

    impl InProcessTransportFactory for ProgressServer {
        fn open(&self) -> BoxFuture<'static, std::io::Result<tokio::io::DuplexStream>> {
            let server = self.clone();
            Box::pin(async move {
                let (client, transport) = tokio::io::duplex(4096);
                tokio::spawn(async move {
                    if let Ok(running) = server.serve(transport).await {
                        let _ = running.waiting().await;
                    }
                });
                Ok(client)
            })
        }
    }

    async fn progress_client() -> anyhow::Result<(
        Arc<RmcpClient>,
        Arc<tokio::sync::Notify>,
        mpsc::UnboundedReceiver<serde_json::Value>,
    )> {
        let (observed_meta, observed) = mpsc::unbounded_channel();
        let release = Arc::new(tokio::sync::Notify::new());
        let client = RmcpClient::new_in_process_client(Arc::new(ProgressServer {
            observed_meta,
            release: release.clone(),
        }))
        .await?;
        client
            .initialize(
                ClientInfo::default().with_protocol_version(ProtocolVersion::V_2025_06_18),
                Some(Duration::from_secs(5)),
                Box::new(|_, _| Box::pin(async { anyhow::bail!("unexpected elicitation") })),
            )
            .await?;
        Ok((Arc::new(client), release, observed))
    }

    #[tokio::test]
    async fn call_tool_adds_progress_token_preserves_meta_and_cleans_up_on_completion()
    -> anyhow::Result<()> {
        let (client, release, mut observed) = progress_client().await?;
        let (tx, mut rx) = mpsc::channel(2);
        let call = client.call_tool_with_progress(
            "render".into(),
            None,
            Some(serde_json::json!({"requestScope": "preserved"})),
            Some(Duration::from_secs(5)),
            Some(tx),
        );
        tokio::pin!(call);
        let progress = tokio::select! {
            result = &mut call => panic!("tool completed before progress: {result:?}"),
            progress = timeout(Duration::from_secs(5), rx.recv()) => progress?.unwrap(),
        };
        assert_eq!(progress.total, Some(1.5));
        let meta = timeout(Duration::from_secs(5), observed.recv())
            .await?
            .unwrap();
        assert_eq!(meta["requestScope"], "preserved");
        assert!(meta["progressToken"].is_number(), "preserve the SDK token");
        assert!(
            meta.get(PROGRESS_ROUTE_META_KEY).is_none(),
            "private route marker must not reach the server"
        );
        release.notify_one();
        timeout(Duration::from_secs(5), call).await??;
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
        Ok(())
    }

    #[test]
    fn overlapping_calls_route_independently_and_drop_unregisters() {
        let registry = ProgressRegistry::default();
        let (tx_a, mut rx_a) = mpsc::channel(2);
        let (tx_b, mut rx_b) = mpsc::channel(2);
        let a = registry.register(tx_a);
        let b = registry.register(tx_b);
        registry.route(notification(&b.token, 0.5, Some(1.5)));
        assert!(rx_a.try_recv().is_err());
        assert_eq!(rx_b.try_recv().unwrap().total, Some(1.5));
        let token = a.token.clone();
        drop(a);
        registry.route(notification(&token, 1.0, Some(2.0)));
        assert!(matches!(
            rx_a.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
        registry.route(notification(&b.token, 1.0, None));
        assert_eq!(rx_b.try_recv().unwrap().total, None);
    }

    #[test]
    fn sdk_token_aliases_are_connection_scoped_and_removed_with_the_call() {
        let registry = ProgressRegistry::default();
        let (tx_a, _rx_a) = mpsc::channel(2);
        let (tx_b, _rx_b) = mpsc::channel(2);
        let a = registry.register(tx_a);
        let b = registry.register(tx_b);
        let connection_a: Arc<Aliases> = Arc::default();
        let connection_b: Arc<Aliases> = Arc::default();
        let token = ProgressToken(NumberOrString::Number(0));
        registry.bind(&connection_a, token.clone(), a.token.clone());
        registry.bind(&connection_b, token.clone(), b.token.clone());
        assert_eq!(connection_a.lock().unwrap().get(&token), Some(&a.token));
        assert_eq!(connection_b.lock().unwrap().get(&token), Some(&b.token));
        let dropped_route = a.token.clone();
        drop(a);
        assert!(connection_a.lock().unwrap().is_empty());
        registry.bind(&connection_a, token.clone(), dropped_route);
        assert!(
            connection_a.lock().unwrap().is_empty(),
            "late sends cannot restore a cancelled route"
        );
        assert_eq!(connection_b.lock().unwrap().get(&token), Some(&b.token));
        drop(b);
        assert!(connection_b.lock().unwrap().is_empty());
    }

    #[test]
    fn invalid_totals_are_indeterminate_and_full_queues_never_block() {
        let registry = ProgressRegistry::default();
        let (tx, mut rx) = mpsc::channel(1);
        let registration = registry.register(tx);
        registry.route(notification(&registration.token, 2.0, Some(-1.0)));
        registry.route(notification(&registration.token, 3.0, Some(4.0)));
        assert_eq!(rx.try_recv().unwrap().total, None);
        assert!(rx.try_recv().is_err());
        registry.route(notification(&registration.token, -1.0, Some(4.0)));
        assert!(rx.try_recv().is_err());
        for progress in [f64::NAN, f64::INFINITY] {
            let mut invalid = notification(&registration.token, 0.0, None);
            invalid.progress = progress;
            registry.route(invalid);
            assert!(rx.try_recv().is_err());
        }
        let mut invalid_total = notification(&registration.token, 1.0, None);
        invalid_total.total = Some(f64::INFINITY);
        registry.route(invalid_total);
        assert_eq!(rx.try_recv().unwrap().total, None);
    }

    #[tokio::test]
    async fn transport_progress_reaches_only_the_registered_call() -> anyhow::Result<()> {
        let registry = ProgressRegistry::default();
        let (tx, mut rx) = mpsc::channel(2);
        let registered = registry.register(tx);
        let service = ElicitationClientService::new(
            ClientInfo::default(),
            Box::new(|_, _| Box::pin(async { anyhow::bail!("unexpected elicitation") })),
            ElicitationPauseState::new(),
        )
        .with_progress(registry);
        let (client_transport, server_transport) = tokio::io::duplex(4096);
        let client = serve_directly(service, client_transport, None);
        let mut server = IntoTransport::<RoleServer, _, _>::into_transport(server_transport);
        for token in ["unregistered", registered.token.as_str()] {
            let message: ServerJsonRpcMessage = serde_json::from_value(serde_json::json!({
                "jsonrpc": "2.0", "method": "notifications/progress",
                "params": {"progressToken": token, "progress": 0.5, "total": 1.5, "message": "rendering"}
            }))?;
            server.send(message).await?;
        }
        let progress = timeout(Duration::from_secs(5), rx.recv()).await?.unwrap();
        assert_eq!(
            progress,
            ToolProgress {
                progress: 0.5,
                total: Some(1.5),
                message: Some("rendering".into())
            }
        );
        assert!(rx.try_recv().is_err());
        client.cancellation_token().cancel();
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_call_scope_drops_its_progress_route() -> anyhow::Result<()> {
        let (client, release, _observed) = progress_client().await?;
        let (tx, mut rx) = mpsc::channel(2);
        let task = tokio::spawn(async move {
            client
                .call_tool_with_progress(
                    "render".into(),
                    None,
                    None,
                    Some(Duration::from_secs(5)),
                    Some(tx),
                )
                .await
        });
        assert_eq!(
            timeout(Duration::from_secs(5), rx.recv())
                .await?
                .unwrap()
                .total,
            Some(1.5)
        );
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
        release.notify_one();
        Ok(())
    }
}
