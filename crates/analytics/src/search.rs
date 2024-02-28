use api_models::analytics::search::{
    GetGlobalSearchRequest, GetSearchRequestWithIndex, GetSearchResponse, OpenMsearchOutput,
    OpensearchOutput, SearchIndex,
};
use aws_config::{self, meta::region::RegionProviderChain, Region};
use common_utils::errors::CustomResult;
use opensearch::{
    auth::Credentials,
    cert::CertificateValidation,
    http::{
        request::JsonBody,
        transport::{SingleNodeConnectionPool, TransportBuilder},
        Url,
    },
    MsearchParts, OpenSearch, SearchParts,
};
use serde_json::{json, Value};
use strum::IntoEnumIterator;

use crate::errors::AnalyticsError;
use crate::OpensearchConfig;

#[derive(Debug, thiserror::Error)]
pub enum OpensearchError {
    #[error("Opensearch connection error")]
    ConnectionError,
    #[error("Opensearch NON-200 response content: '{0}'")]
    ResponseNotOK(String),
    #[error("Opensearch response error")]
    ResponseError,
}

async fn get_opensearch_client(auth: OpensearchConfig) -> Result<OpenSearch, OpensearchError> {
    let transport = match auth {
        OpensearchConfig::Basic {
            host,
            username,
            password,
        } => {
            let url = Url::parse(&host).map_err(|_| OpensearchError::ConnectionError)?;
            let credentials = Credentials::Basic(username, password);
            TransportBuilder::new(SingleNodeConnectionPool::new(url))
                .cert_validation(CertificateValidation::None)
                .auth(credentials)
                .build()
                .map_err(|_| OpensearchError::ConnectionError)?
        }
        OpensearchConfig::Aws { host, region } => {
            let url = Url::parse(&host).map_err(|_| OpensearchError::ConnectionError)?;
            let region_provider = RegionProviderChain::first_try(Region::new(region));
            let sdk_config = aws_config::from_env().region(region_provider).load().await;
            let conn_pool = SingleNodeConnectionPool::new(url);
            TransportBuilder::new(conn_pool)
                .auth(
                    sdk_config
                        .clone()
                        .try_into()
                        .map_err(|_| OpensearchError::ConnectionError)?,
                )
                .service_name("es")
                .build()
                .map_err(|_| OpensearchError::ConnectionError)?
        }
    };
    Ok(OpenSearch::new(transport))
}

pub async fn msearch_results(
    req: GetGlobalSearchRequest,
    merchant_id: &String,
    auth: OpensearchConfig,
) -> CustomResult<Vec<GetSearchResponse>, AnalyticsError> {
    let client = get_opensearch_client(auth)
        .await
        .map_err(|_| AnalyticsError::UnknownError)?;

    let mut msearch_vector: Vec<JsonBody<Value>> = vec![];
    for index in SearchIndex::iter() {
        msearch_vector.push(json!({"index": index.to_string()}).into());
        msearch_vector.push(json!({"query": {"bool": {"must": {"query_string": {"query": req.query}}, "filter": {"match_phrase": {"merchant_id": merchant_id}}}}}).into());
    }

    let response = client
        .msearch(MsearchParts::None)
        .body(msearch_vector)
        .send()
        .await
        .map_err(|_| AnalyticsError::UnknownError)?;

    let response_body = response
        .json::<OpenMsearchOutput<Value>>()
        .await
        .map_err(|_| AnalyticsError::UnknownError)?;

    Ok(response_body
        .responses
        .into_iter()
        .zip(SearchIndex::iter())
        .map(|(index_hit, index)| GetSearchResponse {
            count: index_hit.hits.total.value,
            index: index,
            hits: index_hit
                .hits
                .hits
                .into_iter()
                .map(|hit| hit._source)
                .collect(),
        })
        .collect())
}

pub async fn search_results(
    req: GetSearchRequestWithIndex,
    merchant_id: &String,
    auth: OpensearchConfig,
) -> CustomResult<GetSearchResponse, AnalyticsError> {
    let search_req = req.search_req;

    let client = get_opensearch_client(auth)
        .await
        .map_err(|_| AnalyticsError::UnknownError)?;

    let response = client
        .search(SearchParts::Index(&[&req.index.to_string()]))
        .from(search_req.offset)
        .size(search_req.count)
        .body(json!({"query": {"bool": {"must": {"query_string": {"query": search_req.query}}, "filter": {"match_phrase": {"merchant_id": merchant_id}}}}}))
        .send()
        .await
        .map_err(|_| AnalyticsError::UnknownError)?;

    let response_body = response
        .json::<OpensearchOutput<Value>>()
        .await
        .map_err(|_| AnalyticsError::UnknownError)?;

    Ok(GetSearchResponse {
        count: response_body.hits.total.value,
        index: req.index,
        hits: response_body
            .hits
            .hits
            .into_iter()
            .map(|hit| hit._source)
            .collect(),
    })
}
