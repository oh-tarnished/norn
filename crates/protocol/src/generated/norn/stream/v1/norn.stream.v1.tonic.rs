// @generated
/// Generated client implementations.
pub mod stream_service_client {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    use tonic::codegen::http::Uri;
    /** StreamService manages streams and the long-running work done on their
 stored history: compaction, verification, retention and export. Those return a
 google.longrunning.Operation, polled through the google.longrunning.
 Operations service the daemon also serves.
*/
    #[derive(Debug, Clone)]
    pub struct StreamServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl<T> StreamServiceClient<T>
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
        ) -> StreamServiceClient<InterceptedService<T, F>>
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
            StreamServiceClient::new(InterceptedService::new(inner, interceptor))
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
        pub async fn create_stream(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateStreamRequest>,
        ) -> std::result::Result<tonic::Response<super::Stream>, tonic::Status> {
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
                "/norn.stream.v1.StreamService/CreateStream",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.stream.v1.StreamService", "CreateStream"));
            self.inner.unary(req, path, codec).await
        }
        /** Reads one stream, with its current totals.
*/
        pub async fn get_stream(
            &mut self,
            request: impl tonic::IntoRequest<super::GetStreamRequest>,
        ) -> std::result::Result<tonic::Response<super::Stream>, tonic::Status> {
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
                "/norn.stream.v1.StreamService/GetStream",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.stream.v1.StreamService", "GetStream"));
            self.inner.unary(req, path, codec).await
        }
        /** Lists streams in name order.
*/
        pub async fn list_streams(
            &mut self,
            request: impl tonic::IntoRequest<super::ListStreamsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListStreamsResponse>,
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
                "/norn.stream.v1.StreamService/ListStreams",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.stream.v1.StreamService", "ListStreams"));
            self.inner.unary(req, path, codec).await
        }
        /** Reads one segment file of a stream: its chunks, samples, size and span,
 and with SEGMENT_VIEW_FULL its encodings. Changes nothing. Since 1.8.
*/
        pub async fn get_segment(
            &mut self,
            request: impl tonic::IntoRequest<super::GetSegmentRequest>,
        ) -> std::result::Result<tonic::Response<super::Segment>, tonic::Status> {
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
                "/norn.stream.v1.StreamService/GetSegment",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.stream.v1.StreamService", "GetSegment"));
            self.inner.unary(req, path, codec).await
        }
        /** Lists a stream's segment files, sealed and open, in name order: how its
 history sits on disk. Changes nothing. Since 1.8.
*/
        pub async fn list_segments(
            &mut self,
            request: impl tonic::IntoRequest<super::ListSegmentsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListSegmentsResponse>,
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
                "/norn.stream.v1.StreamService/ListSegments",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.stream.v1.StreamService", "ListSegments"));
            self.inner.unary(req, path, codec).await
        }
        /** Changes the fields named in `update_mask`. Tightening `retention` takes
 effect at the next retention pass, never inside this call.
*/
        pub async fn update_stream(
            &mut self,
            request: impl tonic::IntoRequest<super::UpdateStreamRequest>,
        ) -> std::result::Result<tonic::Response<super::Stream>, tonic::Status> {
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
                "/norn.stream.v1.StreamService/UpdateStream",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.stream.v1.StreamService", "UpdateStream"));
            self.inner.unary(req, path, codec).await
        }
        /** Deletes a stream that no signal records into and that holds no
 samples; FAILED_PRECONDITION with ERROR_REASON_RESOURCE_NOT_EMPTY
 otherwise.
*/
        pub async fn delete_stream(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteStreamRequest>,
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
                "/norn.stream.v1.StreamService/DeleteStream",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.stream.v1.StreamService", "DeleteStream"));
            self.inner.unary(req, path, codec).await
        }
        /** Merges small chunks, re-chooses encodings and rebuilds the index for a
 stream's history (SPEC section 42). Crash-safe and lossless: every
 sample readable before is readable after, bit for bit. Ingestion
 continues meanwhile.
*/
        pub async fn compact_stream(
            &mut self,
            request: impl tonic::IntoRequest<super::CompactStreamRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::super::google::longrunning::Operation>,
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
                "/norn.stream.v1.StreamService/CompactStream",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new("norn.stream.v1.StreamService", "CompactStream"),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Reads and checks every chunk of a stream's sealed segments, through
 whatever startup skipped. Changes no stored byte. Damage found is reported
 in the result and counted in the stream's `corrupt_sample_count`.
*/
        pub async fn verify_stream(
            &mut self,
            request: impl tonic::IntoRequest<super::VerifyStreamRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::super::google::longrunning::Operation>,
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
                "/norn.stream.v1.StreamService/VerifyStream",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.stream.v1.StreamService", "VerifyStream"));
            self.inner.unary(req, path, codec).await
        }
        /** Runs the stream's retention policy now rather than at the next pass (SPEC
 section 43). With `validate_only`, removes nothing and says what would go.
*/
        pub async fn apply_stream_retention(
            &mut self,
            request: impl tonic::IntoRequest<super::ApplyStreamRetentionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::super::google::longrunning::Operation>,
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
                "/norn.stream.v1.StreamService/ApplyStreamRetention",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "norn.stream.v1.StreamService",
                        "ApplyStreamRetention",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Writes a stream's samples in an interval to a file in an interchange
 format (SPEC section 44), streaming and in bounded memory.
*/
        pub async fn export_stream(
            &mut self,
            request: impl tonic::IntoRequest<super::ExportStreamRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::super::google::longrunning::Operation>,
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
                "/norn.stream.v1.StreamService/ExportStream",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.stream.v1.StreamService", "ExportStream"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod stream_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with StreamServiceServer.
    #[async_trait]
    pub trait StreamService: std::marker::Send + std::marker::Sync + 'static {
        async fn create_stream(
            &self,
            request: tonic::Request<super::CreateStreamRequest>,
        ) -> std::result::Result<tonic::Response<super::Stream>, tonic::Status>;
        /** Reads one stream, with its current totals.
*/
        async fn get_stream(
            &self,
            request: tonic::Request<super::GetStreamRequest>,
        ) -> std::result::Result<tonic::Response<super::Stream>, tonic::Status>;
        /** Lists streams in name order.
*/
        async fn list_streams(
            &self,
            request: tonic::Request<super::ListStreamsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListStreamsResponse>,
            tonic::Status,
        >;
        /** Reads one segment file of a stream: its chunks, samples, size and span,
 and with SEGMENT_VIEW_FULL its encodings. Changes nothing. Since 1.8.
*/
        async fn get_segment(
            &self,
            request: tonic::Request<super::GetSegmentRequest>,
        ) -> std::result::Result<tonic::Response<super::Segment>, tonic::Status>;
        /** Lists a stream's segment files, sealed and open, in name order: how its
 history sits on disk. Changes nothing. Since 1.8.
*/
        async fn list_segments(
            &self,
            request: tonic::Request<super::ListSegmentsRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListSegmentsResponse>,
            tonic::Status,
        >;
        /** Changes the fields named in `update_mask`. Tightening `retention` takes
 effect at the next retention pass, never inside this call.
*/
        async fn update_stream(
            &self,
            request: tonic::Request<super::UpdateStreamRequest>,
        ) -> std::result::Result<tonic::Response<super::Stream>, tonic::Status>;
        /** Deletes a stream that no signal records into and that holds no
 samples; FAILED_PRECONDITION with ERROR_REASON_RESOURCE_NOT_EMPTY
 otherwise.
*/
        async fn delete_stream(
            &self,
            request: tonic::Request<super::DeleteStreamRequest>,
        ) -> std::result::Result<tonic::Response<()>, tonic::Status>;
        /** Merges small chunks, re-chooses encodings and rebuilds the index for a
 stream's history (SPEC section 42). Crash-safe and lossless: every
 sample readable before is readable after, bit for bit. Ingestion
 continues meanwhile.
*/
        async fn compact_stream(
            &self,
            request: tonic::Request<super::CompactStreamRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::super::google::longrunning::Operation>,
            tonic::Status,
        >;
        /** Reads and checks every chunk of a stream's sealed segments, through
 whatever startup skipped. Changes no stored byte. Damage found is reported
 in the result and counted in the stream's `corrupt_sample_count`.
*/
        async fn verify_stream(
            &self,
            request: tonic::Request<super::VerifyStreamRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::super::google::longrunning::Operation>,
            tonic::Status,
        >;
        /** Runs the stream's retention policy now rather than at the next pass (SPEC
 section 43). With `validate_only`, removes nothing and says what would go.
*/
        async fn apply_stream_retention(
            &self,
            request: tonic::Request<super::ApplyStreamRetentionRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::super::google::longrunning::Operation>,
            tonic::Status,
        >;
        /** Writes a stream's samples in an interval to a file in an interchange
 format (SPEC section 44), streaming and in bounded memory.
*/
        async fn export_stream(
            &self,
            request: tonic::Request<super::ExportStreamRequest>,
        ) -> std::result::Result<
            tonic::Response<super::super::super::super::google::longrunning::Operation>,
            tonic::Status,
        >;
    }
    /** StreamService manages streams and the long-running work done on their
 stored history: compaction, verification, retention and export. Those return a
 google.longrunning.Operation, polled through the google.longrunning.
 Operations service the daemon also serves.
*/
    #[derive(Debug)]
    pub struct StreamServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> StreamServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for StreamServiceServer<T>
    where
        T: StreamService,
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
                "/norn.stream.v1.StreamService/CreateStream" => {
                    #[allow(non_camel_case_types)]
                    struct CreateStreamSvc<T: StreamService>(pub Arc<T>);
                    impl<
                        T: StreamService,
                    > tonic::server::UnaryService<super::CreateStreamRequest>
                    for CreateStreamSvc<T> {
                        type Response = super::Stream;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateStreamRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as StreamService>::create_stream(&inner, request).await
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
                        let method = CreateStreamSvc(inner);
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
                "/norn.stream.v1.StreamService/GetStream" => {
                    #[allow(non_camel_case_types)]
                    struct GetStreamSvc<T: StreamService>(pub Arc<T>);
                    impl<
                        T: StreamService,
                    > tonic::server::UnaryService<super::GetStreamRequest>
                    for GetStreamSvc<T> {
                        type Response = super::Stream;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetStreamRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as StreamService>::get_stream(&inner, request).await
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
                        let method = GetStreamSvc(inner);
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
                "/norn.stream.v1.StreamService/ListStreams" => {
                    #[allow(non_camel_case_types)]
                    struct ListStreamsSvc<T: StreamService>(pub Arc<T>);
                    impl<
                        T: StreamService,
                    > tonic::server::UnaryService<super::ListStreamsRequest>
                    for ListStreamsSvc<T> {
                        type Response = super::ListStreamsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListStreamsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as StreamService>::list_streams(&inner, request).await
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
                        let method = ListStreamsSvc(inner);
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
                "/norn.stream.v1.StreamService/GetSegment" => {
                    #[allow(non_camel_case_types)]
                    struct GetSegmentSvc<T: StreamService>(pub Arc<T>);
                    impl<
                        T: StreamService,
                    > tonic::server::UnaryService<super::GetSegmentRequest>
                    for GetSegmentSvc<T> {
                        type Response = super::Segment;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetSegmentRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as StreamService>::get_segment(&inner, request).await
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
                        let method = GetSegmentSvc(inner);
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
                "/norn.stream.v1.StreamService/ListSegments" => {
                    #[allow(non_camel_case_types)]
                    struct ListSegmentsSvc<T: StreamService>(pub Arc<T>);
                    impl<
                        T: StreamService,
                    > tonic::server::UnaryService<super::ListSegmentsRequest>
                    for ListSegmentsSvc<T> {
                        type Response = super::ListSegmentsResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListSegmentsRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as StreamService>::list_segments(&inner, request).await
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
                        let method = ListSegmentsSvc(inner);
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
                "/norn.stream.v1.StreamService/UpdateStream" => {
                    #[allow(non_camel_case_types)]
                    struct UpdateStreamSvc<T: StreamService>(pub Arc<T>);
                    impl<
                        T: StreamService,
                    > tonic::server::UnaryService<super::UpdateStreamRequest>
                    for UpdateStreamSvc<T> {
                        type Response = super::Stream;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::UpdateStreamRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as StreamService>::update_stream(&inner, request).await
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
                        let method = UpdateStreamSvc(inner);
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
                "/norn.stream.v1.StreamService/DeleteStream" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteStreamSvc<T: StreamService>(pub Arc<T>);
                    impl<
                        T: StreamService,
                    > tonic::server::UnaryService<super::DeleteStreamRequest>
                    for DeleteStreamSvc<T> {
                        type Response = ();
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteStreamRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as StreamService>::delete_stream(&inner, request).await
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
                        let method = DeleteStreamSvc(inner);
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
                "/norn.stream.v1.StreamService/CompactStream" => {
                    #[allow(non_camel_case_types)]
                    struct CompactStreamSvc<T: StreamService>(pub Arc<T>);
                    impl<
                        T: StreamService,
                    > tonic::server::UnaryService<super::CompactStreamRequest>
                    for CompactStreamSvc<T> {
                        type Response = super::super::super::super::google::longrunning::Operation;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CompactStreamRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as StreamService>::compact_stream(&inner, request).await
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
                        let method = CompactStreamSvc(inner);
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
                "/norn.stream.v1.StreamService/VerifyStream" => {
                    #[allow(non_camel_case_types)]
                    struct VerifyStreamSvc<T: StreamService>(pub Arc<T>);
                    impl<
                        T: StreamService,
                    > tonic::server::UnaryService<super::VerifyStreamRequest>
                    for VerifyStreamSvc<T> {
                        type Response = super::super::super::super::google::longrunning::Operation;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::VerifyStreamRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as StreamService>::verify_stream(&inner, request).await
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
                        let method = VerifyStreamSvc(inner);
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
                "/norn.stream.v1.StreamService/ApplyStreamRetention" => {
                    #[allow(non_camel_case_types)]
                    struct ApplyStreamRetentionSvc<T: StreamService>(pub Arc<T>);
                    impl<
                        T: StreamService,
                    > tonic::server::UnaryService<super::ApplyStreamRetentionRequest>
                    for ApplyStreamRetentionSvc<T> {
                        type Response = super::super::super::super::google::longrunning::Operation;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ApplyStreamRetentionRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as StreamService>::apply_stream_retention(
                                        &inner,
                                        request,
                                    )
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
                        let method = ApplyStreamRetentionSvc(inner);
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
                "/norn.stream.v1.StreamService/ExportStream" => {
                    #[allow(non_camel_case_types)]
                    struct ExportStreamSvc<T: StreamService>(pub Arc<T>);
                    impl<
                        T: StreamService,
                    > tonic::server::UnaryService<super::ExportStreamRequest>
                    for ExportStreamSvc<T> {
                        type Response = super::super::super::super::google::longrunning::Operation;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ExportStreamRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as StreamService>::export_stream(&inner, request).await
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
                        let method = ExportStreamSvc(inner);
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
    impl<T> Clone for StreamServiceServer<T> {
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
    pub const SERVICE_NAME: &str = "norn.stream.v1.StreamService";
    impl<T> tonic::server::NamedService for StreamServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
