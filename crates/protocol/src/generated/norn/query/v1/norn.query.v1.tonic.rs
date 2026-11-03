// @generated
/// Generated client implementations.
pub mod query_service_client {
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
    pub struct QueryServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl<T> QueryServiceClient<T>
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
        ) -> QueryServiceClient<InterceptedService<T, F>>
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
            QueryServiceClient::new(InterceptedService::new(inner, interceptor))
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
        /** Reads samples or aggregates of the selected signals (SPEC section 22).

 Server-streaming so a result of any size flows in bounded memory on
 both sides: the daemon never builds the whole result, and a consumer
 that stops reading stalls the query rather than growing a buffer. Each
 message carries a resume token, so a broken stream continues where it
 stopped instead of starting over.
*/
        pub async fn query_samples(
            &mut self,
            request: impl tonic::IntoRequest<super::QuerySamplesRequest>,
        ) -> std::result::Result<
            tonic::Response<tonic::codec::Streaming<super::QuerySamplesResponse>>,
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
                "/norn.query.v1.QueryService/QuerySamples",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.query.v1.QueryService", "QuerySamples"));
            self.inner.server_streaming(req, path, codec).await
        }
        /** Reads one bounded page of what QuerySamples streams: raw samples over
 an interval, or aggregates over windows, such as the newest sample of
 each signal (AGGREGATE_FUNCTION_LATEST). The rules are QuerySamples'.
 For a caller that wants an answer per call rather than a stream; repeat
 with `next_page_token` to read on, exactly, with nothing lost or
 repeated even when samples arrive between calls.
*/
        pub async fn browse_samples(
            &mut self,
            request: impl tonic::IntoRequest<super::BrowseSamplesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BrowseSamplesResponse>,
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
                "/norn.query.v1.QueryService/BrowseSamples",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.query.v1.QueryService", "BrowseSamples"));
            self.inner.unary(req, path, codec).await
        }
        /** Finds when something happened: each sample passing a predicate, or
 each sample whose value or quality differs from the previous sample of
 its signal. Deterministic: nothing is inferred, ranked or judged.
 Reads at most a bounded number of samples per call; past it, the call
 returns what it found with a `next_page_token`, possibly with no
 events, so a search of a quiet year cannot hold the daemon.
*/
        pub async fn find_events(
            &mut self,
            request: impl tonic::IntoRequest<super::FindEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::FindEventsResponse>,
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
                "/norn.query.v1.QueryService/FindEvents",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.query.v1.QueryService", "FindEvents"));
            self.inner.unary(req, path, codec).await
        }
        /** Computes each selected signal's statistics over an interval: sample
 count, minimum, maximum, average and sum, and its first and last
 sample. Paginated by signal.
*/
        pub async fn compute_stats(
            &mut self,
            request: impl tonic::IntoRequest<super::ComputeStatsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ComputeStatsResponse>,
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
                "/norn.query.v1.QueryService/ComputeStats",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.query.v1.QueryService", "ComputeStats"));
            self.inner.unary(req, path, codec).await
        }
        /** Computes each selected signal's statistics over two intervals, a
 baseline and a comparison, and the change from one to the other, such
 as this week against last. Paginated by signal.
*/
        pub async fn compare_periods(
            &mut self,
            request: impl tonic::IntoRequest<super::ComparePeriodsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ComparePeriodsResponse>,
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
                "/norn.query.v1.QueryService/ComparePeriods",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.query.v1.QueryService", "ComparePeriods"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod query_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with QueryServiceServer.
    #[async_trait]
    pub trait QueryService: std::marker::Send + std::marker::Sync + 'static {
        /// Server streaming response type for the QuerySamples method.
        type QuerySamplesStream: tonic::codegen::tokio_stream::Stream<
                Item = std::result::Result<super::QuerySamplesResponse, tonic::Status>,
            >
            + std::marker::Send
            + 'static;
        /** Reads samples or aggregates of the selected signals (SPEC section 22).

 Server-streaming so a result of any size flows in bounded memory on
 both sides: the daemon never builds the whole result, and a consumer
 that stops reading stalls the query rather than growing a buffer. Each
 message carries a resume token, so a broken stream continues where it
 stopped instead of starting over.
*/
        async fn query_samples(
            &self,
            request: tonic::Request<super::QuerySamplesRequest>,
        ) -> std::result::Result<
            tonic::Response<Self::QuerySamplesStream>,
            tonic::Status,
        >;
        /** Reads one bounded page of what QuerySamples streams: raw samples over
 an interval, or aggregates over windows, such as the newest sample of
 each signal (AGGREGATE_FUNCTION_LATEST). The rules are QuerySamples'.
 For a caller that wants an answer per call rather than a stream; repeat
 with `next_page_token` to read on, exactly, with nothing lost or
 repeated even when samples arrive between calls.
*/
        async fn browse_samples(
            &self,
            request: tonic::Request<super::BrowseSamplesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BrowseSamplesResponse>,
            tonic::Status,
        >;
        /** Finds when something happened: each sample passing a predicate, or
 each sample whose value or quality differs from the previous sample of
 its signal. Deterministic: nothing is inferred, ranked or judged.
 Reads at most a bounded number of samples per call; past it, the call
 returns what it found with a `next_page_token`, possibly with no
 events, so a search of a quiet year cannot hold the daemon.
*/
        async fn find_events(
            &self,
            request: tonic::Request<super::FindEventsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::FindEventsResponse>,
            tonic::Status,
        >;
        /** Computes each selected signal's statistics over an interval: sample
 count, minimum, maximum, average and sum, and its first and last
 sample. Paginated by signal.
*/
        async fn compute_stats(
            &self,
            request: tonic::Request<super::ComputeStatsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ComputeStatsResponse>,
            tonic::Status,
        >;
        /** Computes each selected signal's statistics over two intervals, a
 baseline and a comparison, and the change from one to the other, such
 as this week against last. Paginated by signal.
*/
        async fn compare_periods(
            &self,
            request: tonic::Request<super::ComparePeriodsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ComparePeriodsResponse>,
            tonic::Status,
        >;
    }
    #[derive(Debug)]
    pub struct QueryServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> QueryServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for QueryServiceServer<T>
    where
        T: QueryService,
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
                "/norn.query.v1.QueryService/QuerySamples" => {
                    #[allow(non_camel_case_types)]
                    struct QuerySamplesSvc<T: QueryService>(pub Arc<T>);
                    impl<
                        T: QueryService,
                    > tonic::server::ServerStreamingService<super::QuerySamplesRequest>
                    for QuerySamplesSvc<T> {
                        type Response = super::QuerySamplesResponse;
                        type ResponseStream = T::QuerySamplesStream;
                        type Future = BoxFuture<
                            tonic::Response<Self::ResponseStream>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::QuerySamplesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as QueryService>::query_samples(&inner, request).await
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
                        let method = QuerySamplesSvc(inner);
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
                        let res = grpc.server_streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/norn.query.v1.QueryService/BrowseSamples" => {
                    #[allow(non_camel_case_types)]
                    struct BrowseSamplesSvc<T: QueryService>(pub Arc<T>);
                    impl<
                        T: QueryService,
                    > tonic::server::UnaryService<super::BrowseSamplesRequest>
                    for BrowseSamplesSvc<T> {
                        type Response = super::BrowseSamplesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BrowseSamplesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as QueryService>::browse_samples(&inner, request).await
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
                        let method = BrowseSamplesSvc(inner);
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
                "/norn.query.v1.QueryService/FindEvents" => {
                    #[allow(non_camel_case_types)]
                    struct FindEventsSvc<T: QueryService>(pub Arc<T>);
                    impl<
                        T: QueryService,
                    > tonic::server::UnaryService<super::FindEventsRequest>
                    for FindEventsSvc<T> {
                        type Response = super::FindEventsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::FindEventsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as QueryService>::find_events(&inner, request).await
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
                        let method = FindEventsSvc(inner);
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
                "/norn.query.v1.QueryService/ComputeStats" => {
                    #[allow(non_camel_case_types)]
                    struct ComputeStatsSvc<T: QueryService>(pub Arc<T>);
                    impl<
                        T: QueryService,
                    > tonic::server::UnaryService<super::ComputeStatsRequest>
                    for ComputeStatsSvc<T> {
                        type Response = super::ComputeStatsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ComputeStatsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as QueryService>::compute_stats(&inner, request).await
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
                        let method = ComputeStatsSvc(inner);
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
                "/norn.query.v1.QueryService/ComparePeriods" => {
                    #[allow(non_camel_case_types)]
                    struct ComparePeriodsSvc<T: QueryService>(pub Arc<T>);
                    impl<
                        T: QueryService,
                    > tonic::server::UnaryService<super::ComparePeriodsRequest>
                    for ComparePeriodsSvc<T> {
                        type Response = super::ComparePeriodsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ComparePeriodsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as QueryService>::compare_periods(&inner, request).await
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
                        let method = ComparePeriodsSvc(inner);
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
    impl<T> Clone for QueryServiceServer<T> {
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
    pub const SERVICE_NAME: &str = "norn.query.v1.QueryService";
    impl<T> tonic::server::NamedService for QueryServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
