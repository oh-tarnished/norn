// @generated
/// Generated client implementations.
pub mod entity_service_client {
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
    pub struct EntityServiceClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl<T> EntityServiceClient<T>
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
        ) -> EntityServiceClient<InterceptedService<T, F>>
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
            EntityServiceClient::new(InterceptedService::new(inner, interceptor))
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
        pub async fn create_entity(
            &mut self,
            request: impl tonic::IntoRequest<super::CreateEntityRequest>,
        ) -> std::result::Result<tonic::Response<super::Entity>, tonic::Status> {
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
                "/norn.entity.v1.EntityService/CreateEntity",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.entity.v1.EntityService", "CreateEntity"));
            self.inner.unary(req, path, codec).await
        }
        /** Creates up to 1000 entities in one call, all or none (AIP-233), with one
 durable registry commit for the whole batch.

 Safe to repeat: an entity that already exists is returned as stored, its
 fields left alone, so a client may send its whole catalogue at every
 start.

 It writes, so it is no MCP tool: the hints below say so, and the daemon
 serves only tools hinted read-only.
*/
        pub async fn batch_create_entities(
            &mut self,
            request: impl tonic::IntoRequest<super::BatchCreateEntitiesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BatchCreateEntitiesResponse>,
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
                "/norn.entity.v1.EntityService/BatchCreateEntities",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(
                    GrpcMethod::new(
                        "norn.entity.v1.EntityService",
                        "BatchCreateEntities",
                    ),
                );
            self.inner.unary(req, path, codec).await
        }
        /** Reads one entity.
*/
        pub async fn get_entity(
            &mut self,
            request: impl tonic::IntoRequest<super::GetEntityRequest>,
        ) -> std::result::Result<tonic::Response<super::Entity>, tonic::Status> {
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
                "/norn.entity.v1.EntityService/GetEntity",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.entity.v1.EntityService", "GetEntity"));
            self.inner.unary(req, path, codec).await
        }
        /** Lists entities in name order.
*/
        pub async fn list_entities(
            &mut self,
            request: impl tonic::IntoRequest<super::ListEntitiesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListEntitiesResponse>,
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
                "/norn.entity.v1.EntityService/ListEntities",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.entity.v1.EntityService", "ListEntities"));
            self.inner.unary(req, path, codec).await
        }
        /** Changes the fields named in `update_mask`.
*/
        pub async fn update_entity(
            &mut self,
            request: impl tonic::IntoRequest<super::UpdateEntityRequest>,
        ) -> std::result::Result<tonic::Response<super::Entity>, tonic::Status> {
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
                "/norn.entity.v1.EntityService/UpdateEntity",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.entity.v1.EntityService", "UpdateEntity"));
            self.inner.unary(req, path, codec).await
        }
        /** Deletes an entity that has no signals. FAILED_PRECONDITION with
 ERROR_REASON_RESOURCE_NOT_EMPTY otherwise: deleting an entity never
 deletes history as a side effect.
*/
        pub async fn delete_entity(
            &mut self,
            request: impl tonic::IntoRequest<super::DeleteEntityRequest>,
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
                "/norn.entity.v1.EntityService/DeleteEntity",
            );
            let mut req = request.into_request();
            req.extensions_mut()
                .insert(GrpcMethod::new("norn.entity.v1.EntityService", "DeleteEntity"));
            self.inner.unary(req, path, codec).await
        }
    }
}
/// Generated server implementations.
pub mod entity_service_server {
    #![allow(
        unused_variables,
        dead_code,
        missing_docs,
        clippy::wildcard_imports,
        clippy::let_unit_value,
    )]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with EntityServiceServer.
    #[async_trait]
    pub trait EntityService: std::marker::Send + std::marker::Sync + 'static {
        async fn create_entity(
            &self,
            request: tonic::Request<super::CreateEntityRequest>,
        ) -> std::result::Result<tonic::Response<super::Entity>, tonic::Status>;
        /** Creates up to 1000 entities in one call, all or none (AIP-233), with one
 durable registry commit for the whole batch.

 Safe to repeat: an entity that already exists is returned as stored, its
 fields left alone, so a client may send its whole catalogue at every
 start.

 It writes, so it is no MCP tool: the hints below say so, and the daemon
 serves only tools hinted read-only.
*/
        async fn batch_create_entities(
            &self,
            request: tonic::Request<super::BatchCreateEntitiesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::BatchCreateEntitiesResponse>,
            tonic::Status,
        >;
        /** Reads one entity.
*/
        async fn get_entity(
            &self,
            request: tonic::Request<super::GetEntityRequest>,
        ) -> std::result::Result<tonic::Response<super::Entity>, tonic::Status>;
        /** Lists entities in name order.
*/
        async fn list_entities(
            &self,
            request: tonic::Request<super::ListEntitiesRequest>,
        ) -> std::result::Result<
            tonic::Response<super::ListEntitiesResponse>,
            tonic::Status,
        >;
        /** Changes the fields named in `update_mask`.
*/
        async fn update_entity(
            &self,
            request: tonic::Request<super::UpdateEntityRequest>,
        ) -> std::result::Result<tonic::Response<super::Entity>, tonic::Status>;
        /** Deletes an entity that has no signals. FAILED_PRECONDITION with
 ERROR_REASON_RESOURCE_NOT_EMPTY otherwise: deleting an entity never
 deletes history as a side effect.
*/
        async fn delete_entity(
            &self,
            request: tonic::Request<super::DeleteEntityRequest>,
        ) -> std::result::Result<tonic::Response<()>, tonic::Status>;
    }
    #[derive(Debug)]
    pub struct EntityServiceServer<T> {
        inner: Arc<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
        max_decoding_message_size: Option<usize>,
        max_encoding_message_size: Option<usize>,
    }
    impl<T> EntityServiceServer<T> {
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
    impl<T, B> tonic::codegen::Service<http::Request<B>> for EntityServiceServer<T>
    where
        T: EntityService,
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
                "/norn.entity.v1.EntityService/CreateEntity" => {
                    #[allow(non_camel_case_types)]
                    struct CreateEntitySvc<T: EntityService>(pub Arc<T>);
                    impl<
                        T: EntityService,
                    > tonic::server::UnaryService<super::CreateEntityRequest>
                    for CreateEntitySvc<T> {
                        type Response = super::Entity;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::CreateEntityRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as EntityService>::create_entity(&inner, request).await
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
                        let method = CreateEntitySvc(inner);
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
                "/norn.entity.v1.EntityService/BatchCreateEntities" => {
                    #[allow(non_camel_case_types)]
                    struct BatchCreateEntitiesSvc<T: EntityService>(pub Arc<T>);
                    impl<
                        T: EntityService,
                    > tonic::server::UnaryService<super::BatchCreateEntitiesRequest>
                    for BatchCreateEntitiesSvc<T> {
                        type Response = super::BatchCreateEntitiesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::BatchCreateEntitiesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as EntityService>::batch_create_entities(&inner, request)
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
                        let method = BatchCreateEntitiesSvc(inner);
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
                "/norn.entity.v1.EntityService/GetEntity" => {
                    #[allow(non_camel_case_types)]
                    struct GetEntitySvc<T: EntityService>(pub Arc<T>);
                    impl<
                        T: EntityService,
                    > tonic::server::UnaryService<super::GetEntityRequest>
                    for GetEntitySvc<T> {
                        type Response = super::Entity;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::GetEntityRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as EntityService>::get_entity(&inner, request).await
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
                        let method = GetEntitySvc(inner);
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
                "/norn.entity.v1.EntityService/ListEntities" => {
                    #[allow(non_camel_case_types)]
                    struct ListEntitiesSvc<T: EntityService>(pub Arc<T>);
                    impl<
                        T: EntityService,
                    > tonic::server::UnaryService<super::ListEntitiesRequest>
                    for ListEntitiesSvc<T> {
                        type Response = super::ListEntitiesResponse;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::ListEntitiesRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as EntityService>::list_entities(&inner, request).await
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
                        let method = ListEntitiesSvc(inner);
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
                "/norn.entity.v1.EntityService/UpdateEntity" => {
                    #[allow(non_camel_case_types)]
                    struct UpdateEntitySvc<T: EntityService>(pub Arc<T>);
                    impl<
                        T: EntityService,
                    > tonic::server::UnaryService<super::UpdateEntityRequest>
                    for UpdateEntitySvc<T> {
                        type Response = super::Entity;
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::UpdateEntityRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as EntityService>::update_entity(&inner, request).await
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
                        let method = UpdateEntitySvc(inner);
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
                "/norn.entity.v1.EntityService/DeleteEntity" => {
                    #[allow(non_camel_case_types)]
                    struct DeleteEntitySvc<T: EntityService>(pub Arc<T>);
                    impl<
                        T: EntityService,
                    > tonic::server::UnaryService<super::DeleteEntityRequest>
                    for DeleteEntitySvc<T> {
                        type Response = ();
                        type Future = BoxFuture<
                            tonic::Response<Self::Response>,
                            tonic::Status,
                        >;
                        fn call(
                            &mut self,
                            request: tonic::Request<super::DeleteEntityRequest>,
                        ) -> Self::Future {
                            let inner = Arc::clone(&self.0);
                            let fut = async move {
                                <T as EntityService>::delete_entity(&inner, request).await
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
                        let method = DeleteEntitySvc(inner);
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
    impl<T> Clone for EntityServiceServer<T> {
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
    pub const SERVICE_NAME: &str = "norn.entity.v1.EntityService";
    impl<T> tonic::server::NamedService for EntityServiceServer<T> {
        const NAME: &'static str = SERVICE_NAME;
    }
}
