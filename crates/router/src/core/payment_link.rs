use super::errors::{self, StorageErrorExt};
#[cfg(feature = "olap")]
use crate::{
    core::payments::helpers,
    errors::RouterResponse,
    routes::AppState,
    services,
    types::{domain, storage::enums as storage_enums},
};

pub async fn retrieve_payment_link(
    state: AppState,
    merchant_account: domain::MerchantAccount,
    payment_link_id: String,
) -> RouterResponse<api_models::payments::RetrievePaymentLinkResponse> {
    let db = &*state.store;
    let payment_link_object = db
        .find_payment_link_by_payment_link_id(&payment_link_id, &merchant_account.merchant_id)
        .await
        .to_not_found_response(errors::ApiErrorResponse::PaymentLinkNotFound)?;

    let response = api_models::payments::RetrievePaymentLinkResponse {
        payment_link_id: payment_link_object.payment_link_id,
        payment_id: payment_link_object.payment_id,
        merchant_id: payment_link_object.merchant_id,
        link_to_pay: payment_link_object.link_to_pay,
        amount: payment_link_object.amount,
        currency: payment_link_object.currency,
        created_at: payment_link_object.created_at,
        last_modified_at: payment_link_object.last_modified_at,
    };

    Ok(services::ApplicationResponse::Json(response))
}

pub async fn intiate_payment_link_flow(
    state: AppState,
    merchant_account: domain::MerchantAccount,
    merchant_id: String,
    payment_id: String,
) -> RouterResponse<services::PaymentLinkFormData> {
    let db = &*state.store;
    let payment_intent = db
        .find_payment_intent_by_payment_id_merchant_id(
            &payment_id,
            &merchant_id,
            merchant_account.storage_scheme,
        )
        .await
        .to_not_found_response(errors::ApiErrorResponse::PaymentNotFound)?;

    helpers::validate_payment_status_against_not_allowed_statuses(
        &payment_intent.status,
        &[
            storage_enums::IntentStatus::Cancelled,
            storage_enums::IntentStatus::Succeeded,
            storage_enums::IntentStatus::Processing,
            storage_enums::IntentStatus::RequiresCapture,
            storage_enums::IntentStatus::RequiresMerchantAction,
        ],
        "create",
    )?;

    let body = get_html_body();
    let css_script = get_css();
    let js_script = get_js_script(
        payment_intent.amount.to_string(),
        payment_intent.currency.unwrap_or_default().to_string(),
        merchant_account.publishable_key.unwrap_or_default(),
        payment_intent.client_secret.unwrap_or_default(),
        payment_intent.payment_id,
    );

    let payment_link_data = services::PaymentLinkFormData {
        js_script,
        css_script,
        body,
        base_url: state.conf.server.base_url.clone(),
    };
    Ok(services::ApplicationResponse::PaymenkLinkForm(Box::new(
        payment_link_data,
    )))
}

fn get_js_script(
    amount: String,
    currency: String,
    pub_key: String,
    secret: String,
    payment_id: String,
) -> String {
    format!(
    "
    <script>
        window.__PAYMENT_DETAILS_STR = JSON.stringify({{
            client_secret: '{secret}',
            return_url: 'http://localhost:5500/public/index.html',
            merchant_logo: 'https://upload.wikimedia.org/wikipedia/commons/8/83/Steam_icon_logo.svg',
            merchant: 'Steam',
            amount: '{amount}',
            currency: '{currency}',
            purchased_item: 'Tea',
            payment_id: '{payment_id}'
        }});

        const hyper = Hyper(\"{pub_key}\");
        var widgets = null;

        window.__PAYMENT_DETAILS = {{}};
        try {{
        window.__PAYMENT_DETAILS = JSON.parse(window.__PAYMENT_DETAILS_STR);
        }} catch (error) {{
        console.error(\"Failed to parse payment details\");
        }}


        async function initialize() {{
            var paymentDetails = window.__PAYMENT_DETAILS;
            var client_secret = paymentDetails.client_secret;
            const appearance = {{
               theme: \"default\",
            }};
          
            widgets = hyper.widgets({{
              appearance,
              clientSecret: client_secret,
            }});
          
            const unifiedCheckoutOptions = {{
              layout: \"tabs\",
              wallets: {{
                walletReturnUrl: paymentDetails.return_url,
                style: {{
                  theme: \"dark\",
                  type: \"default\",
                  height: 55,
                }},
              }},
            }};
          
            const unifiedCheckout = widgets.create(\"payment\", unifiedCheckoutOptions);
            unifiedCheckout.mount(\"#unified-checkout\");
          }}
          initialize();

          async function handleSubmit(e) {{
            setLoading(true);
            var paymentDetails = window.__PAYMENT_DETAILS;
            const {{ error, data, status }} = await hyper.confirmPayment({{
              widgets,
              confirmParams: {{
                // Make sure to change this to your payment completion page
                return_url: paymentDetails.return_url,
              }},
            }});
            // This point will only be reached if there is an immediate error occurring while confirming the payment. Otherwise, your customer will be redirected to your `return_url`.
            // For some payment flows such as Sofort, iDEAL, your customer will be redirected to an intermediate page to complete authorization of the payment, and then redirected to the `return_url`.
          
            if (error) {{
              if (error.type === \"validation_error\") {{
                showMessage(error.message);
              }} else {{
                showMessage(\"An unexpected error occurred.\");
              }}
            }} else {{
              const {{ paymentIntent }} = await hyper.retrievePaymentIntent(paymentDetails.client_secret);
              if (paymentIntent && paymentIntent.status) {{
                hide(\"#hyper-checkout-sdk\");
                hide(\"#hyper-checkout-details\");
                show(\"#hyper-checkout-status\");
                show(\"#hyper-footer\");
                showStatus(paymentIntent);
              }}
            }}
          
            setLoading(false);
          }}

          // Fetches the payment status after payment submission
            async function checkStatus() {{
            const clientSecret = new URLSearchParams(window.location.search).get(
                \"payment_intent_client_secret\"
            );
            const res = {{
                showSdk: true,
            }};

            if (!clientSecret) {{
                return res;
            }}

            const {{ paymentIntent }} = await hyper.retrievePaymentIntent(clientSecret);

            if (!paymentIntent || !paymentIntent.status) {{
                return res;
            }}

            showStatus(paymentIntent);
            res.showSdk = false;

            return res;
            }}

            function setPageLoading(showLoader) {{
            if (showLoader) {{
                show(\".page-spinner\");
            }} else {{
                hide(\".page-spinner\");
            }}
            }}

            function setLoading(showLoader) {{
                if (showLoader) {{
                  show(\".spinner\");
                  hide(\"#button-text\");
                }} else {{
                  hide(\".spinner\");
                  show(\"#button-text\");
                }}
              }}
              
              function show(id) {{
                removeClass(id, \"hidden\");
              }}
              function hide(id) {{
                addClass(id, \"hidden\");
              }}
              
              function showMessage(msg) {{
                show(\"#payment-message\");
                addText(\"#payment-message\", msg);
              }}
              function showStatus(paymentDetails) {{
                const status = paymentDetails.status;
                let statusDetails = {{
                  imageSource: \"\",
                  message: \"\",
                  status: status,
                  amountText: \"\",
                  items: [],
                }};
            
                switch (status) {{
                    case \"succeeded\":
                      statusDetails.imageSource = \"http://www.clipartbest.com/cliparts/4ib/oRa/4iboRa7RT.png\";
                      statusDetails.message = \"Payment successful\";
                      statusDetails.status = \"Succeeded\";
                      statusDetails.amountText = new Date(paymentDetails.created).toTimeString();
                
                      // Payment details
                      var amountNode = createItem(\"AMOUNT PAID\", paymentDetails.currency + \" \" + paymentDetails.amount);
                      var paymentId = createItem(\"PAYMENT ID\", paymentDetails.payment_id);
                      // @ts-ignore
                      statusDetails.items.push(amountNode, paymentId);
                      break;
                
                    case \"processing\":
                      statusDetails.imageSource = \"http://www.clipartbest.com/cliparts/4ib/oRa/4iboRa7RT.png\";
                      statusDetails.message = \"Payment in progress\";
                      statusDetails.status = \"Processing\";
                      // Payment details
                      var amountNode = createItem(\"AMOUNT PAID\", paymentDetails.currency + \" \" + paymentDetails.amount);
                      var paymentId = createItem(\"PAYMENT ID\", paymentDetails.payment_id);
                      // @ts-ignore
                      statusDetails.items.push(amountNode, paymentId);
                      break;
                
                    case \"failed\":
                      statusDetails.imageSource = \"\";
                      statusDetails.message = \"Payment failed\";
                      statusDetails.status = \"Failed\";
                      // Payment details
                      var amountNode = createItem(\"AMOUNT PAID\", paymentDetails.currency + \" \" + paymentDetails.amount);
                      var paymentId = createItem(\"PAYMENT ID\", paymentDetails.payment_id);
                      // @ts-ignore
                      statusDetails.items.push(amountNode, paymentId);
                      break;
                
                    case \"cancelled\":
                      statusDetails.imageSource = \"\";
                      statusDetails.message = \"Payment cancelled\";
                      statusDetails.status = \"Cancelled\";
                      // Payment details
                      var amountNode = createItem(\"AMOUNT PAID\", paymentDetails.currency + \" \" + paymentDetails.amount);
                      var paymentId = createItem(\"PAYMENT ID\", paymentDetails.payment_id);
                      // @ts-ignore
                      statusDetails.items.push(amountNode, paymentId);
                      break;
                
                    case \"requires_merchant_action\":
                      statusDetails.imageSource = \"\";
                      statusDetails.message = \"Payment under review\";
                      statusDetails.status = \"Under review\";
                      // Payment details
                      var amountNode = createItem(\"AMOUNT PAID\", paymentDetails.currency + \" \" + paymentDetails.amount);
                      var paymentId = createItem(\"PAYMENT ID\", paymentDetails.payment_id);
                      var paymentId = createItem(\"MESSAGE\", \"Your payment is under review by the merchant.\");
                      // @ts-ignore
                      statusDetails.items.push(amountNode, paymentId);
                      break;
                
                    default:
                      statusDetails.imageSource = \"http://www.clipartbest.com/cliparts/4ib/oRa/4iboRa7RT.png\";
                      statusDetails.message = \"Something went wrong\";
                      statusDetails.status = \"Something went wrong\";
                      // Error details
                      if (typeof paymentDetails.error === \"object\") {{
                        var errorCodeNode = createItem(\"ERROR CODE\", paymentDetails.error.code);
                        var errorMessageNode = createItem(\"ERROR MESSAGE\", paymentDetails.error.message);
                        // @ts-ignore
                        statusDetails.items.push(errorMessageNode, errorCodeNode);
                      }}
                      break;
                  }}

                  // Append status
                    var statusTextNode = document.getElementById(\"status-text\");
                    if (statusTextNode !== null) {{
                        statusTextNode.innerText = statusDetails.message;
                    }}

                    // Append image
                    var statusImageNode = document.getElementById(\"status-img\");
                    if (statusImageNode !== null) {{
                        statusImageNode.src = statusDetails.imageSource;
                    }}

                    // Append status details
                    var statusDateNode = document.getElementById(\"status-date\");
                    if (statusDateNode !== null) {{
                        statusDateNode.innerText = statusDetails.amountText;
                    }}

                    // Append items
                    var statusItemNode = document.getElementById(\"hyper-checkout-status-items\");
                    if (statusItemNode !== null) {{
                        statusDetails.items.map((item) => statusItemNode?.append(item));
                    }}
                }}

                function createItem(heading, value) {{
                    var itemNode = document.createElement(\"div\");
                    itemNode.className = \"hyper-checkout-item\";
                    var headerNode = document.createElement(\"div\");
                    headerNode.className = \"hyper-checkout-item-header\";
                    headerNode.innerText = heading;
                    var valueNode = document.createElement(\"div\");
                    valueNode.className = \"hyper-checkout-item-value\";
                    valueNode.innerText = value;
                    itemNode.append(headerNode);
                    itemNode.append(valueNode);
                    return itemNode;
                  }}
                  
                  function addText(id, msg) {{
                    var element = document.querySelector(id);
                    element.innerText = msg;
                  }}
                  
                  function addClass(id, className) {{
                    var element = document.querySelector(id);
                    element.classList.add(className);
                  }}
                  
                  function removeClass(id, className) {{
                    var element = document.querySelector(id);
                    element.classList.remove(className);
                  }}
                  
                  function renderPaymentDetails() {{
                    var paymentDetails = window.__PAYMENT_DETAILS;
                  
                    // Payment details header
                    var paymentDetailsHeaderNode = document.createElement(\"div\");
                    paymentDetailsHeaderNode.className = \"hyper-checkout-details-header\";
                    paymentDetailsHeaderNode.innerText = \"Payment request for \" + paymentDetails.merchant;
                  
                    // Payment details
                    var purchasedItemNode = createItem(\"PAYMENT FOR\", paymentDetails.purchased_item);
                    var paymentIdNode = createItem(\"PAYMENT ID\", paymentDetails.payment_id);
                    var orderAmountNode = createItem(\"AMOUNT PAYABLE\", paymentDetails.currency + \" \" + paymentDetails.amount);
                  
                    // Append to PaymentDetails node
                    var paymentDetailsNode = document.getElementById(\"hyper-checkout-details\");
                    if (paymentDetailsNode !== null) {{
                      paymentDetailsNode.append(paymentDetailsHeaderNode);
                      paymentDetailsNode.append(purchasedItemNode);
                      paymentDetailsNode.append(paymentIdNode);
                      paymentDetailsNode.append(orderAmountNode);
                    }}
                  }}
                  
                  function renderSDKHeader() {{
                    var paymentDetails = window.__PAYMENT_DETAILS;
                  
                    // SDK header's logo
                    var sdkHeaderLogoNode = document.createElement(\"div\");
                    sdkHeaderLogoNode.className = \"hyper-checkout-sdk-header-logo\";
                    var sdkHeaderLogoImageNode = document.createElement(\"img\");
                    sdkHeaderLogoImageNode.src = paymentDetails.merchant_logo;
                    sdkHeaderLogoImageNode.alt = paymentDetails.merchant;
                    sdkHeaderLogoNode.append(sdkHeaderLogoImageNode);
                  
                    // SDK headers' items
                    var sdkHeaderItemNode = document.createElement(\"div\");
                    sdkHeaderItemNode.className = \"hyper-checkout-sdk-items\";
                    var sdkHeaderMerchantNameNode = document.createElement(\"div\");
                    sdkHeaderMerchantNameNode.className = \"hyper-checkout-sdk-header-brand-name\";
                    sdkHeaderMerchantNameNode.innerText = paymentDetails.merchant;
                    var sdkHeaderAmountNode = document.createElement(\"div\");
                    sdkHeaderAmountNode.className = \"hyper-checkout-sdk-header-amount\";
                    sdkHeaderAmountNode.innerText = paymentDetails.currency + \" \" + paymentDetails.amount;
                    sdkHeaderItemNode.append(sdkHeaderMerchantNameNode);
                    sdkHeaderItemNode.append(sdkHeaderAmountNode);
                  
                    // Append to SDK header's node
                    var sdkHeaderNode = document.getElementById(\"hyper-checkout-sdk-header\");
                    if (sdkHeaderNode !== null) {{
                      sdkHeaderNode.append(sdkHeaderLogoNode);
                      sdkHeaderNode.append(sdkHeaderItemNode);
                    }}
                  }}
                  
                  function showSDK(e) {{
                    setPageLoading(true);
                    checkStatus().then((res) => {{
                      if (res.showSdk) {{
                        renderPaymentDetails();
                        renderSDKHeader();
                        show(\"#hyper-checkout-sdk\");
                        show(\"#hyper-checkout-details\")
                      }} else {{
                        show(\"#hyper-checkout-status\");
                        show(\"#hyper-footer\");
                      }}
                    }}).catch((err) => {{
                  
                    }}).finally(() => {{
                      setPageLoading(false);
                    }})
                  }}
    </script>
    ")
}

fn get_html_body() -> String {
    r#" 
        <body onload="showSDK()">
        <div class="page-spinner hidden" id="page-spinner"></div>
        <div class="hyper-checkout">
            <div class="main hidden" id="hyper-checkout-status">
            <div class="hyper-checkout-status-header">
                <img id="status-img" />
                <div id="status-details">
                <div id="status-text"></div>
                <div id="status-date"></div>
                </div>
            </div>
            <div id="hyper-checkout-status-items"></div>
            </div>
            <div class="main hidden" id="hyper-checkout-details"></div>
            <div class="hyper-checkout-sdk hidden" id="hyper-checkout-sdk">
            <div id="hyper-checkout-sdk-header"></div>
            <div id="payment-form-wrap">
                <form id="payment-form" onsubmit="handleSubmit(); return false;">
                <div id="unified-checkout">
                    <!--HyperLoader injects the Unified Checkout-->
                </div>
                <button id="submit" class="checkoutButton payNow">
                    <div class="spinner hidden" id="spinner"></div>
                    <span id="button-text">Pay now</span>
                </button>
                <div id="payment-message" class="hidden"></div>
                </form>
            </div>
            </div>
        </div>
        <div id="hyper-footer" class="hidden">
            <svg class="fill-current " height="18px" width="130px" transform="">
            <path opacity="0.4"
                d="M0.791016 11.7578H1.64062V9.16992H1.71875C2.00684 9.73145 2.63672 10.0928 3.35938 10.0928C4.69727 10.0928 5.56641 9.02344 5.56641 7.37305V7.36328C5.56641 5.72266 4.69238 4.64355 3.35938 4.64355C2.62695 4.64355 2.04102 4.99023 1.71875 5.57617H1.64062V4.73633H0.791016V11.7578ZM3.16406 9.34082C2.20703 9.34082 1.62109 8.58887 1.62109 7.37305V7.36328C1.62109 6.14746 2.20703 5.39551 3.16406 5.39551C4.12598 5.39551 4.69727 6.1377 4.69727 7.36328V7.37305C4.69727 8.59863 4.12598 9.34082 3.16406 9.34082ZM8.85762 10.0928C10.3566 10.0928 11.2844 9.05762 11.2844 7.37305V7.36328C11.2844 5.67383 10.3566 4.64355 8.85762 4.64355C7.35859 4.64355 6.43086 5.67383 6.43086 7.36328V7.37305C6.43086 9.05762 7.35859 10.0928 8.85762 10.0928ZM8.85762 9.34082C7.86152 9.34082 7.3 8.61328 7.3 7.37305V7.36328C7.3 6.11816 7.86152 5.39551 8.85762 5.39551C9.85371 5.39551 10.4152 6.11816 10.4152 7.36328V7.37305C10.4152 8.61328 9.85371 9.34082 8.85762 9.34082ZM13.223 10H14.0727L15.2445 5.92773H15.3227L16.4994 10H17.3539L18.8285 4.73633H17.9838L16.9486 8.94531H16.8705L15.6938 4.73633H14.8881L13.7113 8.94531H13.6332L12.598 4.73633H11.7484L13.223 10ZM21.7047 10.0928C22.9449 10.0928 23.6969 9.38965 23.8775 8.67676L23.8873 8.6377H23.0377L23.0182 8.68164C22.8766 8.99902 22.4371 9.33594 21.7242 9.33594C20.7867 9.33594 20.1861 8.70117 20.1617 7.6123H23.9508V7.28027C23.9508 5.70801 23.0816 4.64355 21.651 4.64355C20.2203 4.64355 19.2926 5.75684 19.2926 7.38281V7.3877C19.2926 9.03809 20.2008 10.0928 21.7047 10.0928ZM21.6461 5.40039C22.4225 5.40039 22.9986 5.89355 23.0865 6.93359H20.1764C20.2691 5.93262 20.8648 5.40039 21.6461 5.40039ZM25.0691 10H25.9188V6.73828C25.9188 5.9668 26.4949 5.4541 27.3055 5.4541C27.491 5.4541 27.6521 5.47363 27.8279 5.50293V4.67773C27.7449 4.66309 27.5643 4.64355 27.4031 4.64355C26.6902 4.64355 26.1971 4.96582 25.9969 5.51758H25.9188V4.73633H25.0691V10ZM30.6797 10.0928C31.9199 10.0928 32.6719 9.38965 32.8525 8.67676L32.8623 8.6377H32.0127L31.9932 8.68164C31.8516 8.99902 31.4121 9.33594 30.6992 9.33594C29.7617 9.33594 29.1611 8.70117 29.1367 7.6123H32.9258V7.28027C32.9258 5.70801 32.0566 4.64355 30.626 4.64355C29.1953 4.64355 28.2676 5.75684 28.2676 7.38281V7.3877C28.2676 9.03809 29.1758 10.0928 30.6797 10.0928ZM30.6211 5.40039C31.3975 5.40039 31.9736 5.89355 32.0615 6.93359H29.1514C29.2441 5.93262 29.8398 5.40039 30.6211 5.40039ZM35.9875 10.0928C36.7199 10.0928 37.3059 9.74609 37.6281 9.16016H37.7062V10H38.5559V2.64648H37.7062V5.56641H37.6281C37.34 5.00488 36.7102 4.64355 35.9875 4.64355C34.6496 4.64355 33.7805 5.71289 33.7805 7.36328V7.37305C33.7805 9.01367 34.6545 10.0928 35.9875 10.0928ZM36.1828 9.34082C35.2209 9.34082 34.6496 8.59863 34.6496 7.37305V7.36328C34.6496 6.1377 35.2209 5.39551 36.1828 5.39551C37.1398 5.39551 37.7258 6.14746 37.7258 7.36328V7.37305C37.7258 8.58887 37.1398 9.34082 36.1828 9.34082ZM45.2164 10.0928C46.5494 10.0928 47.4234 9.01367 47.4234 7.37305V7.36328C47.4234 5.71289 46.5543 4.64355 45.2164 4.64355C44.4938 4.64355 43.8639 5.00488 43.5758 5.56641H43.4977V2.64648H42.648V10H43.4977V9.16016H43.5758C43.898 9.74609 44.484 10.0928 45.2164 10.0928ZM45.0211 9.34082C44.0641 9.34082 43.4781 8.58887 43.4781 7.37305V7.36328C43.4781 6.14746 44.0641 5.39551 45.0211 5.39551C45.983 5.39551 46.5543 6.1377 46.5543 7.36328V7.37305C46.5543 8.59863 45.983 9.34082 45.0211 9.34082ZM48.7957 11.8457C49.7283 11.8457 50.1629 11.5039 50.5975 10.3223L52.6531 4.73633H51.7596L50.3191 9.06738H50.241L48.7957 4.73633H47.8875L49.8357 10.0049L49.7381 10.3174C49.5477 10.9229 49.2547 11.1426 48.7713 11.1426C48.6541 11.1426 48.5223 11.1377 48.4197 11.1182V11.8164C48.5369 11.8359 48.6834 11.8457 48.7957 11.8457Z"
                fill="currentColor"></path>
            <g opacity="0.6">
                <path
                d="M78.42 6.9958C78.42 9.15638 77.085 10.4444 75.2379 10.4444C74.2164 10.4444 73.3269 10.0276 72.9206 9.33816V12.9166H71.4929V3.65235H72.8018L72.9193 4.66772C73.3256 3.97825 74.189 3.5225 75.2366 3.5225C77.017 3.5225 78.4186 4.75861 78.4186 6.9971L78.42 6.9958ZM76.94 6.9958C76.94 5.62985 76.1288 4.78328 74.9492 4.78328C73.8232 4.77029 72.9598 5.62855 72.9598 7.00878C72.9598 8.38901 73.8246 9.18235 74.9492 9.18235C76.0739 9.18235 76.94 8.36304 76.94 6.9958Z"
                fill="currentColor"></path>
                <path
                d="M86.0132 7.3736H80.8809C80.9071 8.62268 81.7313 9.2732 82.7789 9.2732C83.564 9.2732 84.2197 8.90834 84.494 8.17992H85.9479C85.5939 9.53288 84.3895 10.4444 82.7528 10.4444C80.749 10.4444 79.4271 9.06545 79.4271 6.96978C79.4271 4.87412 80.749 3.50818 82.7397 3.50818C84.7305 3.50818 86.0132 4.83517 86.0132 6.83994V7.3736ZM80.894 6.38419H84.5594C84.481 5.226 83.709 4.6404 82.7397 4.6404C81.7705 4.6404 80.9985 5.226 80.894 6.38419Z"
                fill="currentColor"></path>
                <path
                d="M88.5407 3.65204C87.8745 3.65204 87.335 4.18829 87.335 4.85048V10.3156H88.7758V5.22703C88.7758 5.06213 88.9104 4.92709 89.0776 4.92709H91.2773V3.65204H88.5407Z"
                fill="currentColor"></path> -
                <path
                d="M69.1899 3.63908L67.3442 9.17039L65.3535 3.65207H63.8082L66.3606 10.2247C66.439 10.4325 66.4782 10.6026 66.4782 10.7713C66.4782 10.8635 66.469 10.9479 66.4533 11.0258L66.4494 11.0401C66.4403 11.0817 66.4298 11.1206 66.4168 11.1583L66.3201 11.5102C66.2966 11.5971 66.2169 11.6569 66.1268 11.6569H64.0956V12.9189H65.5755C66.5709 12.9189 67.3952 12.6852 67.8667 11.3829L70.6817 3.65207L69.1886 3.63908H69.1899Z"
                fill="currentColor"></path>
                <path
                d="M57 10.3144H58.4264V6.72299C58.4264 5.60375 59.0417 4.82339 60.1807 4.82339C61.1761 4.81041 61.7913 5.396 61.7913 6.68404V10.3144H63.2191V6.46201C63.2191 4.18457 61.8188 3.50809 60.5478 3.50809C59.5785 3.50809 58.8196 3.88593 58.4264 4.51047V0.919022H57V10.3144Z"
                fill="currentColor"></path>
                <path
                d="M93.1623 8.29808C93.1753 8.98755 93.8167 9.39136 94.6945 9.39136C95.5723 9.39136 96.0948 9.06545 96.0948 8.47986C96.0948 7.97218 95.8336 7.69951 95.0733 7.58135L93.7253 7.34763C92.4164 7.1269 91.9057 6.44912 91.9057 5.49997C91.9057 4.30282 93.097 3.52246 94.6161 3.52246C96.2529 3.52246 97.4442 4.30282 97.4572 5.63111H96.0439C96.0308 4.95463 95.4417 4.57679 94.6174 4.57679C93.7932 4.57679 93.3347 4.90269 93.3347 5.44933C93.3347 5.93105 93.6756 6.15178 94.4215 6.28162L95.7434 6.51534C96.987 6.73607 97.563 7.34763 97.563 8.35002C97.563 9.72895 96.2803 10.4457 94.722 10.4457C92.9546 10.4457 91.7633 9.60041 91.7372 8.29808H93.1649H93.1623Z"
                fill="currentColor"></path>
                <path
                d="M100.808 8.75352L102.327 3.652H103.82L105.313 8.75352L106.583 3.652H108.089L106.191 10.3155H104.58L103.061 5.23997L101.529 10.3155H99.9052L97.9941 3.652H99.5002L100.809 8.75352H100.808Z"
                fill="currentColor"></path>
                <path d="M108.926 0.918945H110.511V2.40305H108.926V0.918945ZM109.005 3.65214H110.431V10.3157H109.005V3.65214Z"
                fill="currentColor"></path>
                <path
                d="M119.504 4.7452C118.391 4.7452 117.632 5.55152 117.632 6.9707C117.632 8.46779 118.417 9.19621 119.465 9.19621C120.302 9.19621 120.919 8.72748 121.193 7.84325H122.712C122.371 9.45719 121.141 10.4466 119.491 10.4466C117.502 10.4466 116.165 9.06767 116.165 6.972C116.165 4.87634 117.5 3.51039 119.504 3.51039C121.141 3.51039 122.358 4.43487 122.712 6.04752H121.167C120.932 5.21523 120.289 4.7465 119.504 4.7465V4.7452Z"
                fill="currentColor"></path>
                <path
                d="M113.959 9.05208C113.875 9.05208 113.809 8.98456 113.809 8.90276V4.91399H115.367V3.65191H113.809V1.86787H112.382V3.02607C112.382 3.44287 112.252 3.65062 111.833 3.65062H111.256V4.91269H112.382V8.50414C112.382 9.66234 113.024 10.3128 114.189 10.3128H115.354V9.05078H113.96L113.959 9.05208Z"
                fill="currentColor"></path>
                <path
                d="M127.329 3.50801C126.359 3.50801 125.601 3.88585 125.207 4.5104V0.918945H123.781V10.3144H125.207V6.72292C125.207 5.60367 125.823 4.82332 126.962 4.82332C127.957 4.81033 128.572 5.39592 128.572 6.68397V10.3144H130V6.46193C130 4.18449 128.6 3.50801 127.329 3.50801Z"
                fill="currentColor"></path>
            </g>
            </svg>
        </div>
        </body>
    "#.to_owned()
}

fn get_css() -> String {
    r#"
    html, body {
        height: 100%;
      }
      
      body {
        display: flex;
        flex-flow: column;
        align-items: center;
        justify-content: flex-start;
        margin: 0;
        background-color: #fafafa;
        color: #292929;
      }
      
      .hidden {
        display: none !important;
      }
      
      .hyper-checkout {
        display: flex;
        background-color: #fafafa;
        margin-top: 50px;
      }
      
      .main {
        padding: 15px 15px 15px 25px;
        display: flex;
        flex-flow: column;
        background-color: #fdfdfd;
        margin: 20px 0;
        box-shadow: 0px 1px 10px #f2f2f2;
        width: 500px;
      }
      
      .hyper-checkout-details-header {
        font-weight: 600;
        font-size: 23px;
        font-family: "Montserrat";
      }
      
      .hyper-checkout-item {
        margin-top: 20px;
      }
      
      .hyper-checkout-item-header {
        font-family: "Montserrat";
        font-weight: 500;
        font-size: 12px;
        color: #53655c;
      }
      
      .hyper-checkout-item-value {
        margin-top: 2px;
        font-family: "Montserrat";
        font-weight: 500;
        font-size: 18px;
      }
      
      .hyper-checkout-item-amount {
        font-weight: 600;
        font-size: 23px;
      }
      
      .hyper-checkout-sdk {
        z-index: 2;
        background-color: #fdfdfd;
        margin: 20px 30px 20px 0;
        box-shadow: 0px 1px 10px #f2f2f2;
      }
      
      #hyper-checkout-sdk-header {
        padding: 10px 10px 10px 22px;
        display: flex;
        align-items: flex-start;
        justify-content: flex-start;
        border-bottom: 1px solid #f2f2f2;
      }
      
      .hyper-checkout-sdk-header-logo {
        height: 60px;
        width: 60px;
        background-color: white;
        border-radius: 2px;
      }
      
      .hyper-checkout-sdk-header-logo>img {
        height: 56px;
        width: 56px;
        margin: 2px;
      }
      
      .hyper-checkout-sdk-header-items {
        display: flex;
        flex-flow: column;
        color: white;
        font-size: 20px;
        font-weight: 700;
      }
      
      .hyper-checkout-sdk-items {
        margin-left: 10px;
      }
      
      .hyper-checkout-sdk-header-brand-name,
      .hyper-checkout-sdk-header-amount {
        font-size: 18px;
        font-weight: 600;
        display: flex;
        align-items: center;
        font-family: "Montserrat";
        justify-self: flex-start;
      }
      
      .hyper-checkout-sdk-header-amount {
        font-weight: 800;
        font-size: 25px;
      }
      
      .payNow {
        margin-top: 10px;
      }
      
      .checkoutButton {
        height: 48px;
        border-radius: 25px;
        width: 100%;
        border: transparent;
        background: #006df9;
        color: #ffffff;
        font-weight: 600;
        cursor: pointer;
      }
      
      .page-spinner,
      .page-spinner::before,
      .page-spinner::after,
      .spinner,
      .spinner:before,
      .spinner:after {
        border-radius: 50%;
      }
      
      .page-spinner,
      .spinner {
        color: #ffffff;
        font-size: 22px;
        text-indent: -99999px;
        margin: 0px auto;
        position: relative;
        width: 20px;
        height: 20px;
        box-shadow: inset 0 0 0 2px;
        -webkit-transform: translateZ(0);
        -ms-transform: translateZ(0);
        transform: translateZ(0);
      }
      
      .page-spinner::before,
      .page-spinner::after,
      .spinner:before,
      .spinner:after {
        position: absolute;
        content: "";
      }
      
      .page-spinner {
        color: #006df9 !important;
        height: 50px !important;
        width: 50px !important;
        box-shadow: inset 0 0 0 4px !important;
        margin: auto !important;
      }
      
      #hyper-checkout-status {
        margin: 40px !important;
      }
      
      .hyper-checkout-status-header {
        display: flex;
        align-items: center;
        font-family: "Montserrat";
        font-size: 24px;
        font-weight: 600;
      }
      
      #status-img {
        height: 70px;
      }
      
      #status-date {
        font-size: 13px;
        font-weight: 500;
        color: #53655c;
      }
      
      #status-details {
        margin-left: 10px;
        justify-content: center;
        display: flex;
        flex-flow: column;
      }
      
      @keyframes loading {
        0% {
          -webkit-transform: rotate(0deg);
          transform: rotate(0deg);
        }
      
        100% {
          -webkit-transform: rotate(360deg);
          transform: rotate(360deg);
        }
      }
      
      .spinner:before {
        width: 10.4px;
        height: 20.4px;
        background: #016df9;
        border-radius: 20.4px 0 0 20.4px;
        top: -0.2px;
        left: -0.2px;
        -webkit-transform-origin: 10.4px 10.2px;
        transform-origin: 10.4px 10.2px;
        -webkit-animation: loading 2s infinite ease 1.5s;
        animation: loading 2s infinite ease 1.5s;
      }
      
      #payment-message {
        font-size: 12px;
        font-weight: 500;
        padding: 2%;
        color: #ff0000;
        font-family: "Montserrat";
      }
      
      .spinner:after {
        width: 10.4px;
        height: 10.2px;
        background: #016df9;
        border-radius: 0 10.2px 10.2px 0;
        top: -0.1px;
        left: 10.2px;
        -webkit-transform-origin: 0px 10.2px;
        transform-origin: 0px 10.2px;
        -webkit-animation: loading 2s infinite ease;
        animation: loading 2s infinite ease;
      }
      
      #payment-form-wrap {
        margin: 30px;
      }
      
      #payment-form-wrap {
        margin: 30px;
      }
      
      #payment-form {
        max-width: 560px;
        width: 100%;
        margin: 0 auto;
        text-align: center;
      }
      
      @media only screen and (max-width: 765px) {
        .checkoutButton {
          width: 95%;
        }
      
        .hyper-checkout {
          flex-flow: column;
          margin: 0;
          flex-direction: column-reverse;
        }
      
        .main {
          width: auto;
        }
      
        .hyper-checkout-sdk {
          margin: 0;
        }
      
        #hyper-checkout-status {
          padding: 15px;
        }
      
        #status-img {
          height: 60px;
        }
      
        #status-text {
          font-size: 19px;
        }
      
        #status-date {
          font-size: 12px;
        }
      
        .hyper-checkout-item-header {
          font-size: 11px;
        }
      
        .hyper-checkout-item-value {
          font-size: 17px;
        }
      }
    "#
    .to_owned()
}
