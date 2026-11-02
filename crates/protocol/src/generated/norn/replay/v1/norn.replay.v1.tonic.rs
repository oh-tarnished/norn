// @generated
/// Generated client implementations.
pub mod replay_service_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    #[derive(Debug, Clone)]
    pub struct ReplayServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl<T> ReplayServiceClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + std::marker::Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + std::marker::Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> ReplayServiceClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<
                    <T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody,
                >,
            >,
            <T as tonic::codegen::Service<
                http::Request<tonic::body::Body>,
            >>::Error: Into<StdError> + std::marker::Send + std::marker::Sync,
        {
            ReplayServiceClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_decoding_message_size(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.inner = self.inner.max_encoding_message_size(limit);
            self
        }
        pub async fn create_replay_session(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/norn.replay.v1.ReplayService/CreateReplaySession",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "norn.replay.v1.ReplayService",
                        "CreateReplaySession",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Reads a session, including its clock.
*/
        pub async fn get_replay_session(
            &mut self,
            request: impl tonic::IntoRequest<super::GetReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/norn.replay.v1.ReplayService/GetReplaySession",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("norn.replay.v1.ReplayService", "GetReplaySession"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Lists sessions in creation order.
*/
        pub async fn list_replay_sessions(
            &mut self,
            request: impl tonic::IntoRequest<super::ListReplaySessionsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListReplaySessionsResponse>,
            tonic::Status,
        > {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/norn.replay.v1.ReplayService/ListReplaySessions",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("norn.replay.v1.ReplayService", "ListReplaySessions"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Changes the session's pace, the only mutable setting. Subscribers see
 CLOCK_EVENT_KIND_SPEED_CHANGED.
*/
        pub async fn update_replay_session(
            &mut self,
            request: impl tonic::IntoRequest<super::UpdateReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/norn.replay.v1.ReplayService/UpdateReplaySession",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "norn.replay.v1.ReplayService",
                        "UpdateReplaySession",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Deletes a session, stopping it first if it is not STOPPED.
*/
        pub async fn delete_replay_session(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<()>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/norn.replay.v1.ReplayService/DeleteReplaySession",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "norn.replay.v1.ReplayService",
                        "DeleteReplaySession",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Stops the clock: RUNNING becomes PAUSED. Pausing a PAUSED session is a
 no-op, so the method is idempotent.
*/
        pub async fn pause_replay_session(
            &mut self,
            request: impl tonic::IntoRequest<super::PauseReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/norn.replay.v1.ReplayService/PauseReplaySession",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("norn.replay.v1.ReplayService", "PauseReplaySession"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Starts the clock: PAUSED becomes RUNNING. Resuming a RUNNING session is
 a no-op; resuming an ENDED or STOPPED one is a state conflict.
*/
        pub async fn resume_replay_session(
            &mut self,
            request: impl tonic::IntoRequest<super::ResumeReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/norn.replay.v1.ReplayService/ResumeReplaySession",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "norn.replay.v1.ReplayService",
                        "ResumeReplaySession",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Moves the clock to a time within the interval without emitting the
 samples skipped. The session keeps its state, except that an ENDED
 session becomes PAUSED.
*/
        pub async fn seek_replay_session(
            &mut self,
            request: impl tonic::IntoRequest<super::SeekReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/norn.replay.v1.ReplayService/SeekReplaySession",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("norn.replay.v1.ReplayService", "SeekReplaySession"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Advances a PAUSED clock by a number of events or a span of virtual
 time, emitting what it passes, and leaves it PAUSED.
*/
        pub async fn step_replay_session(
            &mut self,
            request: impl tonic::IntoRequest<super::StepReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/norn.replay.v1.ReplayService/StepReplaySession",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("norn.replay.v1.ReplayService", "StepReplaySession"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Ends the session for good: subscribers receive CLOCK_EVENT_KIND_STOPPED
 and their streams close. Stopping a STOPPED session is a no-op.
*/
        pub async fn stop_replay_session(
            &mut self,
            request: impl tonic::IntoRequest<super::StopReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status> {
            self.inner
                .ready()
                .await
                .map_err(|e| {
                    tonic::Status::unknown(
                        format!("Service was not ready: {}", e.into()),
                    )
                })?;
            let codec = tonic_prost::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static(
                "/norn.replay.v1.ReplayService/StopReplaySession",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("norn.replay.v1.ReplayService", "StopReplaySession"),
                );
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod replay_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with ReplayServiceServer.
    #[async_trait]
    pub trait ReplayService: std::marker::Send + std::marker::Sync + 'static {
        async fn create_replay_session(
            &self,
            request: tonic::Request<super::CreateReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status>;
        /** Reads a session, including its clock.
*/
        async fn get_replay_session(
            &self,
            request: tonic::Request<super::GetReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status>;
        /** Lists sessions in creation order.
*/
        async fn list_replay_sessions(
            &self,
            request: tonic::Request<super::ListReplaySessionsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListReplaySessionsResponse>,
            tonic::Status,
        >;
        /** Changes the session's pace, the only mutable setting. Subscribers see
 CLOCK_EVENT_KIND_SPEED_CHANGED.
*/
        async fn update_replay_session(
            &self,
            request: tonic::Request<super::UpdateReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status>;
        /** Deletes a session, stopping it first if it is not STOPPED.
*/
        async fn delete_replay_session(
            &self,
            request: tonic::Request<super::DeleteReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<()>, tonic::Status>;
        /** Stops the clock: RUNNING becomes PAUSED. Pausing a PAUSED session is a
 no-op, so the method is idempotent.
*/
        async fn pause_replay_session(
            &self,
            request: tonic::Request<super::PauseReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status>;
        /** Starts the clock: PAUSED becomes RUNNING. Resuming a RUNNING session is
 a no-op; resuming an ENDED or STOPPED one is a state conflict.
*/
        async fn resume_replay_session(
            &self,
            request: tonic::Request<super::ResumeReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status>;
        /** Moves the clock to a time within the interval without emitting the
 samples skipped. The session keeps its state, except that an ENDED
 session becomes PAUSED.
*/
        async fn seek_replay_session(
            &self,
            request: tonic::Request<super::SeekReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status>;
        /** Advances a PAUSED clock by a number of events or a span of virtual
 time, emitting what it passes, and leaves it PAUSED.
*/
        async fn step_replay_session(
            &self,
            request: tonic::Request<super::StepReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status>;
        /** Ends the session for good: subscribers receive CLOCK_EVENT_KIND_STOPPED
 and their streams close. Stopping a STOPPED session is a no-op.
*/
        async fn stop_replay_session(
            &self,
            request: tonic::Request<super::StopReplaySessionRequest>,
        ) -> std::result::Result<tonic::Response<super::ReplaySession>, tonic::Status>;
    }
    #[derive(Debug)]
    pub struct ReplayServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> ReplayServiceServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
                max_decoding_message_size: None,
                max_encoding_message_size: None,
            }
        }
        pub fn with_interceptor<F>(
            inner: T,
            interceptor: F,
        ) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
        /// Limits the maximum size of a decoded message.
        ///
        /// Default: `4MB`
        #[must_use]
        pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
            self.max_decoding_message_size = Some(limit);
            self
        }
        /// Limits the maximum size of an encoded message.
        ///
        /// Default: `usize::MAX`
        #[must_use]
        pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
            self.max_encoding_message_size = Some(limit);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for ReplayServiceServer<T>
    where
        T: ReplayService,
        B: Body + std::marker::Send + 'static,
        B::Error: Into<StdError> + std::marker::Send + 'static,
    {
        type Response = http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(
            &mut self,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            match req.uri().path() {
                "/norn.replay.v1.ReplayService/CreateReplaySession" => {
                    #[allow(non_camel_case_types)]
                    struct CreateReplaySessionSvc<T: ReplayService>(pub Arc<T>);
                    impl<
                        T: ReplayService,
                    > tonic::server::UnaryService<super::CreateReplaySessionRequest>
                    for CreateReplaySessionSvc<T> {
                        type Response = super::ReplaySession;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateReplaySessionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ReplayService>::create_replay_session(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = CreateReplaySessionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/norn.replay.v1.ReplayService/GetReplaySession" => {
                    #[allow(non_camel_case_types)]
                    struct GetReplaySessionSvc<T: ReplayService>(pub Arc<T>);
                    impl<
                        T: ReplayService,
                    > tonic::server::UnaryService<super::GetReplaySessionRequest>
                    for GetReplaySessionSvc<T> {
                        type Response = super::ReplaySession;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetReplaySessionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ReplayService>::get_replay_session(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = GetReplaySessionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/norn.replay.v1.ReplayService/ListReplaySessions" => {
                    #[allow(non_camel_case_types)]
                    struct ListReplaySessionsSvc<T: ReplayService>(pub Arc<T>);
                    impl<
                        T: ReplayService,
                    > tonic::server::UnaryService<super::ListReplaySessionsRequest>
                    for ListReplaySessionsSvc<T> {
                        type Response = super::ListReplaySessionsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListReplaySessionsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ReplayService>::list_replay_sessions(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ListReplaySessionsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/norn.replay.v1.ReplayService/UpdateReplaySession" => {
                    #[allow(non_camel_case_types)]
                    struct UpdateReplaySessionSvc<T: ReplayService>(pub Arc<T>);
                    impl<
                        T: ReplayService,
                    > tonic::server::UnaryService<super::UpdateReplaySessionRequest>
                    for UpdateReplaySessionSvc<T> {
                        type Response = super::ReplaySession;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::UpdateReplaySessionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ReplayService>::update_replay_session(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = UpdateReplaySessionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/norn.replay.v1.ReplayService/DeleteReplaySession" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteReplaySessionSvc<T: ReplayService>(pub Arc<T>);
                    impl<
                        T: ReplayService,
                    > tonic::server::UnaryService<super::DeleteReplaySessionRequest>
                    for DeleteReplaySessionSvc<T> {
                        type Response = ();
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteReplaySessionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ReplayService>::delete_replay_session(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = DeleteReplaySessionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/norn.replay.v1.ReplayService/PauseReplaySession" => {
                    #[allow(non_camel_case_types)]
                    struct PauseReplaySessionSvc<T: ReplayService>(pub Arc<T>);
                    impl<
                        T: ReplayService,
                    > tonic::server::UnaryService<super::PauseReplaySessionRequest>
                    for PauseReplaySessionSvc<T> {
                        type Response = super::ReplaySession;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::PauseReplaySessionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ReplayService>::pause_replay_session(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = PauseReplaySessionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/norn.replay.v1.ReplayService/ResumeReplaySession" => {
                    #[allow(non_camel_case_types)]
                    struct ResumeReplaySessionSvc<T: ReplayService>(pub Arc<T>);
                    impl<
                        T: ReplayService,
                    > tonic::server::UnaryService<super::ResumeReplaySessionRequest>
                    for ResumeReplaySessionSvc<T> {
                        type Response = super::ReplaySession;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ResumeReplaySessionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ReplayService>::resume_replay_session(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = ResumeReplaySessionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/norn.replay.v1.ReplayService/SeekReplaySession" => {
                    #[allow(non_camel_case_types)]
                    struct SeekReplaySessionSvc<T: ReplayService>(pub Arc<T>);
                    impl<
                        T: ReplayService,
                    > tonic::server::UnaryService<super::SeekReplaySessionRequest>
                    for SeekReplaySessionSvc<T> {
                        type Response = super::ReplaySession;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::SeekReplaySessionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ReplayService>::seek_replay_session(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = SeekReplaySessionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/norn.replay.v1.ReplayService/StepReplaySession" => {
                    #[allow(non_camel_case_types)]
                    struct StepReplaySessionSvc<T: ReplayService>(pub Arc<T>);
                    impl<
                        T: ReplayService,
                    > tonic::server::UnaryService<super::StepReplaySessionRequest>
                    for StepReplaySessionSvc<T> {
                        type Response = super::ReplaySession;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::StepReplaySessionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ReplayService>::step_replay_session(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = StepReplaySessionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/norn.replay.v1.ReplayService/StopReplaySession" => {
                    #[allow(non_camel_case_types)]
                    struct StopReplaySessionSvc<T: ReplayService>(pub Arc<T>);
                    impl<
                        T: ReplayService,
                    > tonic::server::UnaryService<super::StopReplaySessionRequest>
                    for StopReplaySessionSvc<T> {
                        type Response = super::ReplaySession;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::StopReplaySessionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as ReplayService>::stop_replay_session(&inner, request)
                                    .await
                            };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let max_decoding_message_size = self.max_decoding_message_size;
                    let max_encoding_message_size = self.max_encoding_message_size;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let method = StopReplaySessionSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(
                                accept_compression_encodings,
                                send_compression_encodings,
                            )
                            .apply_max_message_size_config(
                                max_decoding_message_size,
                                max_encoding_message_size,
                            );
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => {
                    Box::pin(async move {
                        let mut response = http::Response::new(
                            tonic::body::Body::default(),
                        );
                        let headers = response.headers_mut();
                        headers
                            .insert(
                                tonic::Status::GRPC_STATUS,
                                (tonic::Code::Unimplemented as i32).into(),
                            );
                        headers
                            .insert(
                                http::header::CONTENT_TYPE,
                                tonic::metadata::GRPC_CONTENT_TYPE,
                            );
                        Ok(response)
                    })
                }
            }
        }
    }
    impl<T> Clone for ReplayServiceServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
                max_decoding_message_size: self.max_decoding_message_size,
                max_encoding_message_size: self.max_encoding_message_size,
            }
        }
    }
    /// Generated gRPC service name
    pub const SERVICE_NAME: &str = "norn.replay.v1.ReplayService";
    impl<T> tonic::server::NamedService for ReplayServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
