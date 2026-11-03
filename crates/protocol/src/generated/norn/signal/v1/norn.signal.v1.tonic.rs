// @generated
/// Generated client implementations.
pub mod signal_service_client {
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
    pub struct SignalServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl<T> SignalServiceClient<T>
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
        ) -> SignalServiceClient<InterceptedService<T, F>>
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
            SignalServiceClient::new(InterceptedService::new(inner, interceptor))
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
        pub async fn create_signal(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateSignalRequest>,
        ) -> std::result::Result<tonic::Response<super::Signal>, tonic::Status> {
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
                "/norn.signal.v1.SignalService/CreateSignal",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.signal.v1.SignalService", "CreateSignal"));
            self.inner.unary(req, path, codec).await
        }
        /** Creates up to 1000 signals in one call, all or none (AIP-233), with one
 durable registry commit for the whole batch: provisioning a deployment's tens
 of thousands of signals without a call and a commit each.

 Safe to repeat: a signal that already exists with the same value type and
 stream is returned as stored, its other fields left alone, so a client
 may send its whole catalogue at every start. ALREADY_EXISTS only if one
 exists with another value type or stream; nothing is created then.

 It writes, so it is no MCP tool: the hints below say so, and the daemon
 serves only tools hinted read-only.
*/
        pub async fn batch_create_signals(
            &mut self,
            request: impl tonic::IntoRequest<super::BatchCreateSignalsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BatchCreateSignalsResponse>,
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
                "/norn.signal.v1.SignalService/BatchCreateSignals",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("norn.signal.v1.SignalService", "BatchCreateSignals"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Reads one signal.
*/
        pub async fn get_signal(
            &mut self,
            request: impl tonic::IntoRequest<super::GetSignalRequest>,
        ) -> std::result::Result<tonic::Response<super::Signal>, tonic::Status> {
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
                "/norn.signal.v1.SignalService/GetSignal",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.signal.v1.SignalService", "GetSignal"));
            self.inner.unary(req, path, codec).await
        }
        /** Lists the signals of one entity, or of every entity with the AIP-159
 wildcard parent `entities/-`.
*/
        pub async fn list_signals(
            &mut self,
            request: impl tonic::IntoRequest<super::ListSignalsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListSignalsResponse>,
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
                "/norn.signal.v1.SignalService/ListSignals",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.signal.v1.SignalService", "ListSignals"));
            self.inner.unary(req, path, codec).await
        }
        /** Changes the fields named in `update_mask`. Immutable fields cannot be
 named.
*/
        pub async fn update_signal(
            &mut self,
            request: impl tonic::IntoRequest<super::UpdateSignalRequest>,
        ) -> std::result::Result<tonic::Response<super::Signal>, tonic::Status> {
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
                "/norn.signal.v1.SignalService/UpdateSignal",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.signal.v1.SignalService", "UpdateSignal"));
            self.inner.unary(req, path, codec).await
        }
        /** Deletes a signal that has no stored samples. FAILED_PRECONDITION with
 ERROR_REASON_RESOURCE_NOT_EMPTY otherwise: removing history is the
 stream's retention policy's job, never a side effect.
*/
        pub async fn delete_signal(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteSignalRequest>,
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
                "/norn.signal.v1.SignalService/DeleteSignal",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.signal.v1.SignalService", "DeleteSignal"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod signal_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with SignalServiceServer.
    #[async_trait]
    pub trait SignalService: std::marker::Send + std::marker::Sync + 'static {
        async fn create_signal(
            &self,
            request: tonic::Request<super::CreateSignalRequest>,
        ) -> std::result::Result<tonic::Response<super::Signal>, tonic::Status>;
        /** Creates up to 1000 signals in one call, all or none (AIP-233), with one
 durable registry commit for the whole batch: provisioning a deployment's tens
 of thousands of signals without a call and a commit each.

 Safe to repeat: a signal that already exists with the same value type and
 stream is returned as stored, its other fields left alone, so a client
 may send its whole catalogue at every start. ALREADY_EXISTS only if one
 exists with another value type or stream; nothing is created then.

 It writes, so it is no MCP tool: the hints below say so, and the daemon
 serves only tools hinted read-only.
*/
        async fn batch_create_signals(
            &self,
            request: tonic::Request<super::BatchCreateSignalsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BatchCreateSignalsResponse>,
            tonic::Status,
        >;
        /** Reads one signal.
*/
        async fn get_signal(
            &self,
            request: tonic::Request<super::GetSignalRequest>,
        ) -> std::result::Result<tonic::Response<super::Signal>, tonic::Status>;
        /** Lists the signals of one entity, or of every entity with the AIP-159
 wildcard parent `entities/-`.
*/
        async fn list_signals(
            &self,
            request: tonic::Request<super::ListSignalsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListSignalsResponse>,
            tonic::Status,
        >;
        /** Changes the fields named in `update_mask`. Immutable fields cannot be
 named.
*/
        async fn update_signal(
            &self,
            request: tonic::Request<super::UpdateSignalRequest>,
        ) -> std::result::Result<tonic::Response<super::Signal>, tonic::Status>;
        /** Deletes a signal that has no stored samples. FAILED_PRECONDITION with
 ERROR_REASON_RESOURCE_NOT_EMPTY otherwise: removing history is the
 stream's retention policy's job, never a side effect.
*/
        async fn delete_signal(
            &self,
            request: tonic::Request<super::DeleteSignalRequest>,
        ) -> std::result::Result<tonic::Response<()>, tonic::Status>;
    }
    #[derive(Debug)]
    pub struct SignalServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> SignalServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for SignalServiceServer<T>
    where
        T: SignalService,
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
                "/norn.signal.v1.SignalService/CreateSignal" => {
                    #[allow(non_camel_case_types)]
                    struct CreateSignalSvc<T: SignalService>(pub Arc<T>);
                    impl<
                        T: SignalService,
                    > tonic::server::UnaryService<super::CreateSignalRequest>
                    for CreateSignalSvc<T> {
                        type Response = super::Signal;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateSignalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SignalService>::create_signal(&inner, request).await
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
                        let method = CreateSignalSvc(inner);
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
                "/norn.signal.v1.SignalService/BatchCreateSignals" => {
                    #[allow(non_camel_case_types)]
                    struct BatchCreateSignalsSvc<T: SignalService>(pub Arc<T>);
                    impl<
                        T: SignalService,
                    > tonic::server::UnaryService<super::BatchCreateSignalsRequest>
                    for BatchCreateSignalsSvc<T> {
                        type Response = super::BatchCreateSignalsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BatchCreateSignalsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SignalService>::batch_create_signals(&inner, request)
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
                        let method = BatchCreateSignalsSvc(inner);
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
                "/norn.signal.v1.SignalService/GetSignal" => {
                    #[allow(non_camel_case_types)]
                    struct GetSignalSvc<T: SignalService>(pub Arc<T>);
                    impl<
                        T: SignalService,
                    > tonic::server::UnaryService<super::GetSignalRequest>
                    for GetSignalSvc<T> {
                        type Response = super::Signal;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetSignalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SignalService>::get_signal(&inner, request).await
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
                        let method = GetSignalSvc(inner);
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
                "/norn.signal.v1.SignalService/ListSignals" => {
                    #[allow(non_camel_case_types)]
                    struct ListSignalsSvc<T: SignalService>(pub Arc<T>);
                    impl<
                        T: SignalService,
                    > tonic::server::UnaryService<super::ListSignalsRequest>
                    for ListSignalsSvc<T> {
                        type Response = super::ListSignalsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListSignalsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SignalService>::list_signals(&inner, request).await
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
                        let method = ListSignalsSvc(inner);
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
                "/norn.signal.v1.SignalService/UpdateSignal" => {
                    #[allow(non_camel_case_types)]
                    struct UpdateSignalSvc<T: SignalService>(pub Arc<T>);
                    impl<
                        T: SignalService,
                    > tonic::server::UnaryService<super::UpdateSignalRequest>
                    for UpdateSignalSvc<T> {
                        type Response = super::Signal;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::UpdateSignalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SignalService>::update_signal(&inner, request).await
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
                        let method = UpdateSignalSvc(inner);
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
                "/norn.signal.v1.SignalService/DeleteSignal" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteSignalSvc<T: SignalService>(pub Arc<T>);
                    impl<
                        T: SignalService,
                    > tonic::server::UnaryService<super::DeleteSignalRequest>
                    for DeleteSignalSvc<T> {
                        type Response = ();
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteSignalRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as SignalService>::delete_signal(&inner, request).await
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
                        let method = DeleteSignalSvc(inner);
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
    impl<T> Clone for SignalServiceServer<T> {
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
    pub const SERVICE_NAME: &str = "norn.signal.v1.SignalService";
    impl<T> tonic::server::NamedService for SignalServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
