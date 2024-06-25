use common_utils::{
    crypto::{Encryptable, GcmAes256},
    errors::ReportSwitchExt,
    ext_traits::OptionExt,
};
use error_stack::{report, ResultExt};
use masking::{ExposeInterface, Secret};
use router_env::{instrument, tracing};
use rustc_hash::FxHashMap;

use crate::{
    core::{
        errors::{self, StorageErrorExt},
        payment_methods::cards,
    },
    pii::PeekInterface,
    routes::{metrics, SessionState},
    services,
    types::{
        api::customers,
        domain::{
            self,
            types::{self, AsyncLift, TypeEncryption},
            Identifier,
        },
        storage::{self, enums},
    },
    utils::CustomerAddress,
};

pub const REDACTED: &str = "Redacted";

#[instrument(skip(state))]
pub async fn create_customer(
    state: SessionState,
    merchant_account: domain::MerchantAccount,
    key_store: domain::MerchantKeyStore,
    mut customer_data: customers::CustomerRequest,
) -> errors::CustomerResponse<customers::CustomerResponse> {
    let db = state.store.as_ref();
    let customer_id = customer_data
        .customer_id
        .clone()
        .unwrap_or_else(common_utils::generate_customer_id_of_default_length);

    let merchant_id = &merchant_account.merchant_id;
    merchant_id.clone_into(&mut customer_data.merchant_id);

    // We first need to validate whether the customer with the given customer id already exists
    // this may seem like a redundant db call, as the insert_customer will anyway return this error
    //
    // Consider a scenario where the address is inserted and then when inserting the customer,
    // it errors out, now the address that was inserted is not deleted
    match db
        .find_customer_by_customer_id_merchant_id(
            &state,
            &customer_id,
            merchant_id,
            &key_store,
            merchant_account.storage_scheme,
        )
        .await
    {
        Err(err) => {
            if !err.current_context().is_db_not_found() {
                Err(err).switch()
            } else {
                Ok(())
            }
        }
        Ok(_) => Err(report!(
            errors::CustomersErrorResponse::CustomerAlreadyExists
        )),
    }?;

    let key = key_store.key.get_inner().peek();
    let address = if let Some(addr) = &customer_data.address {
        let customer_address: api_models::payments::AddressDetails = addr.clone();

        let address = customer_data
            .get_domain_address(
                &state,
                customer_address,
                merchant_id,
                &customer_id,
                key,
                merchant_account.storage_scheme,
            )
            .await
            .switch()
            .attach_printable("Failed while encrypting address")?;

        Some(
            db.insert_address_for_customers(&state, address, &key_store)
                .await
                .switch()
                .attach_printable("Failed while inserting new address")?,
        )
    } else {
        None
    };

    let identifier = Identifier::Merchant(key_store.merchant_id.clone());
    let mut map = FxHashMap::default();
    map.insert("name".to_string(), customer_data.name.clone());
    map.insert("phone".to_string(), customer_data.phone.clone());
    let encrypted_data = types::batch_encrypt_optional(&state, map, identifier.clone(), key)
        .await
        .switch()
        .attach_printable("Failed while encrypting Customer")?;
    let new_customer = async {
        Ok(domain::Customer {
            customer_id: customer_id.to_owned(),
            merchant_id: merchant_id.to_string(),
            name: customer_data
                .name
                .and_then(|_| encrypted_data.get("name").cloned()),
            email: customer_data
                .email
                .async_lift(|inner| {
                    types::encrypt_optional(
                        &state,
                        inner.map(|inner| inner.expose()),
                        identifier.clone(),
                        key,
                    )
                })
                .await?,
            phone: customer_data
                .phone
                .and_then(|_| encrypted_data.get("phone").cloned()),
            description: customer_data.description,
            phone_country_code: customer_data.phone_country_code,
            metadata: customer_data.metadata,
            id: None,
            connector_customer: None,
            address_id: address.clone().map(|addr| addr.address_id),
            created_at: common_utils::date_time::now(),
            modified_at: common_utils::date_time::now(),
            default_payment_method_id: None,
            updated_by: None,
        })
    }
    .await
    .switch()
    .attach_printable("Failed while encrypting Customer")?;

    let customer = db
        .insert_customer(
            &state,
            new_customer,
            &key_store,
            merchant_account.storage_scheme,
        )
        .await
        .to_duplicate_response(errors::CustomersErrorResponse::CustomerAlreadyExists)?;

    let address_details = address.map(api_models::payments::AddressDetails::from);

    Ok(services::ApplicationResponse::Json(
        customers::CustomerResponse::from((customer, address_details)),
    ))
}

#[instrument(skip(state))]
pub async fn retrieve_customer(
    state: SessionState,
    merchant_account: domain::MerchantAccount,
    key_store: domain::MerchantKeyStore,
    req: customers::CustomerId,
) -> errors::CustomerResponse<customers::CustomerResponse> {
    let db = state.store.as_ref();
    let response = db
        .find_customer_by_customer_id_merchant_id(
            &state,
            &req.customer_id,
            &merchant_account.merchant_id,
            &key_store,
            merchant_account.storage_scheme,
        )
        .await
        .switch()?;
    let address = match &response.address_id {
        Some(address_id) => Some(api_models::payments::AddressDetails::from(
            db.find_address_by_address_id(&state, address_id, &key_store)
                .await
                .switch()?,
        )),
        None => None,
    };
    Ok(services::ApplicationResponse::Json(
        customers::CustomerResponse::from((response, address)),
    ))
}

#[instrument(skip(state))]
pub async fn list_customers(
    state: SessionState,
    merchant_id: String,
    key_store: domain::MerchantKeyStore,
) -> errors::CustomerResponse<Vec<customers::CustomerResponse>> {
    let db = state.store.as_ref();

    let domain_customers = db
        .list_customers_by_merchant_id(&state, &merchant_id, &key_store)
        .await
        .switch()?;

    let customers = domain_customers
        .into_iter()
        .map(|domain_customer| customers::CustomerResponse::from((domain_customer, None)))
        .collect();

    Ok(services::ApplicationResponse::Json(customers))
}

#[instrument(skip_all)]
pub async fn delete_customer(
    state: SessionState,
    merchant_account: domain::MerchantAccount,
    req: customers::CustomerId,
    key_store: domain::MerchantKeyStore,
) -> errors::CustomerResponse<customers::CustomerDeleteResponse> {
    let db = &state.store;

    let customer_orig = db
        .find_customer_by_customer_id_merchant_id(
            &state,
            &req.customer_id,
            &merchant_account.merchant_id,
            &key_store,
            merchant_account.storage_scheme,
        )
        .await
        .switch()?;

    let customer_mandates = db
        .find_mandate_by_merchant_id_customer_id(&merchant_account.merchant_id, &req.customer_id)
        .await
        .switch()?;

    for mandate in customer_mandates.into_iter() {
        if mandate.mandate_status == enums::MandateStatus::Active {
            Err(errors::CustomersErrorResponse::MandateActive)?
        }
    }

    match db
        .find_payment_method_by_customer_id_merchant_id_list(
            &req.customer_id,
            &merchant_account.merchant_id,
            None,
        )
        .await
    {
        // check this in review
        Ok(customer_payment_methods) => {
            for pm in customer_payment_methods.into_iter() {
                if pm.payment_method == Some(enums::PaymentMethod::Card) {
                    cards::delete_card_from_locker(
                        &state,
                        &req.customer_id,
                        &merchant_account.merchant_id,
                        pm.locker_id.as_ref().unwrap_or(&pm.payment_method_id),
                    )
                    .await
                    .switch()?;
                }
                db.delete_payment_method_by_merchant_id_payment_method_id(
                    &merchant_account.merchant_id,
                    &pm.payment_method_id,
                )
                .await
                .switch()?;
            }
        }
        Err(error) => {
            if error.current_context().is_db_not_found() {
                Ok(())
            } else {
                Err(error)
                    .change_context(errors::CustomersErrorResponse::InternalServerError)
                    .attach_printable("failed find_payment_method_by_customer_id_merchant_id_list")
            }?
        }
    };

    let key = key_store.key.get_inner().peek();
    let identifier = Identifier::Merchant(key_store.merchant_id.clone());

    let redacted_encrypted_value: Encryptable<Secret<_>> = Encryptable::encrypt_via_api(
        &state,
        REDACTED.to_string().into(),
        identifier.clone(),
        key,
        GcmAes256,
    )
    .await
    .switch()?;

    let redacted_encrypted_email: Encryptable<Secret<_, common_utils::pii::EmailStrategy>> =
        Encryptable::encrypt_via_api(
            &state,
            REDACTED.to_string().into(),
            identifier.clone(),
            key,
            GcmAes256,
        )
        .await
        .switch()?;

    let update_address = storage::AddressUpdate::Update {
        city: Some(REDACTED.to_string()),
        country: None,
        line1: Some(redacted_encrypted_value.clone()),
        line2: Some(redacted_encrypted_value.clone()),
        line3: Some(redacted_encrypted_value.clone()),
        state: Some(redacted_encrypted_value.clone()),
        zip: Some(redacted_encrypted_value.clone()),
        first_name: Some(redacted_encrypted_value.clone()),
        last_name: Some(redacted_encrypted_value.clone()),
        phone_number: Some(redacted_encrypted_value.clone()),
        country_code: Some(REDACTED.to_string()),
        updated_by: merchant_account.storage_scheme.to_string(),
        email: Some(redacted_encrypted_email),
    };

    match db
        .update_address_by_merchant_id_customer_id(
            &state,
            &req.customer_id,
            &merchant_account.merchant_id,
            update_address,
            &key_store,
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(error) => {
            if error.current_context().is_db_not_found() {
                Ok(())
            } else {
                Err(error)
                    .change_context(errors::CustomersErrorResponse::InternalServerError)
                    .attach_printable("failed update_address_by_merchant_id_customer_id")
            }
        }
    }?;

    let updated_customer = storage::CustomerUpdate::Update {
        name: Some(redacted_encrypted_value.clone()),
        email: Some(
            Encryptable::encrypt_via_api(
                &state,
                REDACTED.to_string().into(),
                identifier,
                key,
                GcmAes256,
            )
            .await
            .switch()?,
        ),
        phone: Box::new(Some(redacted_encrypted_value.clone())),
        description: Some(REDACTED.to_string()),
        phone_country_code: Some(REDACTED.to_string()),
        metadata: None,
        connector_customer: None,
        address_id: None,
    };
    db.update_customer_by_customer_id_merchant_id(
        &state,
        req.customer_id.clone(),
        merchant_account.merchant_id,
        customer_orig,
        updated_customer,
        &key_store,
        merchant_account.storage_scheme,
    )
    .await
    .switch()?;

    let response = customers::CustomerDeleteResponse {
        customer_id: req.customer_id,
        customer_deleted: true,
        address_deleted: true,
        payment_methods_deleted: true,
    };
    metrics::CUSTOMER_REDACTED.add(&metrics::CONTEXT, 1, &[]);
    Ok(services::ApplicationResponse::Json(response))
}

#[instrument(skip(state))]
pub async fn update_customer(
    state: SessionState,
    merchant_account: domain::MerchantAccount,
    update_customer: customers::CustomerRequest,
    key_store: domain::MerchantKeyStore,
) -> errors::CustomerResponse<customers::CustomerResponse> {
    let db = state.store.as_ref();
    //Add this in update call if customer can be updated anywhere else

    let customer_id = update_customer
        .customer_id
        .as_ref()
        .get_required_value("customer_id")
        .change_context(errors::CustomersErrorResponse::InternalServerError)
        .attach("Missing required field `customer_id`")?;

    let customer = db
        .find_customer_by_customer_id_merchant_id(
            &state,
            customer_id,
            &merchant_account.merchant_id,
            &key_store,
            merchant_account.storage_scheme,
        )
        .await
        .switch()?;

    let key = key_store.key.get_inner().peek();

    let address = if let Some(addr) = &update_customer.address {
        match customer.address_id.clone() {
            Some(address_id) => {
                let customer_address: api_models::payments::AddressDetails = addr.clone();
                let update_address = update_customer
                    .get_address_update(
                        &state,
                        customer_address,
                        key,
                        merchant_account.storage_scheme,
                        merchant_account.merchant_id.clone(),
                    )
                    .await
                    .switch()
                    .attach_printable("Failed while encrypting Address while Update")?;
                Some(
                    db.update_address(&state, address_id, update_address, &key_store)
                        .await
                        .switch()
                        .attach_printable(format!(
                            "Failed while updating address: merchant_id: {}, customer_id: {:?}",
                            merchant_account.merchant_id, customer_id
                        ))?,
                )
            }
            None => {
                let customer_address: api_models::payments::AddressDetails = addr.clone();

                let address = update_customer
                    .get_domain_address(
                        &state,
                        customer_address,
                        &merchant_account.merchant_id,
                        &customer.customer_id,
                        key,
                        merchant_account.storage_scheme,
                    )
                    .await
                    .switch()
                    .attach_printable("Failed while encrypting address")?;
                Some(
                    db.insert_address_for_customers(&state, address, &key_store)
                        .await
                        .switch()
                        .attach_printable("Failed while inserting new address")?,
                )
            }
        }
    } else {
        match &customer.address_id {
            Some(address_id) => Some(
                db.find_address_by_address_id(&state, address_id, &key_store)
                    .await
                    .switch()?,
            ),
            None => None,
        }
    };

    let identifier = Identifier::Merchant(key_store.merchant_id.clone());
    let mut map = FxHashMap::default();
    map.insert("name".to_string(), update_customer.name.clone());
    map.insert("phone".to_string(), update_customer.phone.clone());
    let encrypted_data = types::batch_encrypt_optional(&state, map, identifier.clone(), key)
        .await
        .switch()?;
    let response = db
        .update_customer_by_customer_id_merchant_id(
            &state,
            customer_id.to_owned(),
            merchant_account.merchant_id.to_owned(),
            customer,
            async {
                Ok(storage::CustomerUpdate::Update {
                    name: update_customer
                        .name
                        .and_then(|_| encrypted_data.get("name").cloned()),
                    email: update_customer
                        .email
                        .async_lift(|inner| {
                            types::encrypt_optional(
                                &state,
                                inner.map(|inner| inner.expose()),
                                identifier.clone(),
                                key,
                            )
                        })
                        .await?,
                    phone: Box::new(
                        update_customer
                            .phone
                            .and_then(|_| encrypted_data.get("phone").cloned()),
                    ),
                    phone_country_code: update_customer.phone_country_code,
                    metadata: update_customer.metadata,
                    description: update_customer.description,
                    connector_customer: None,
                    address_id: address.clone().map(|addr| addr.address_id),
                })
            }
            .await
            .switch()
            .attach_printable("Failed while encrypting while updating customer")?,
            &key_store,
            merchant_account.storage_scheme,
        )
        .await
        .switch()?;

    Ok(services::ApplicationResponse::Json(
        customers::CustomerResponse::from((response, update_customer.address)),
    ))
}
