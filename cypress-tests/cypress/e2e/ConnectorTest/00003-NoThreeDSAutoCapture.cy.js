import confirmBody from "../../fixtures/confirm-body.json";
import createConfirmPaymentBody from "../../fixtures/create-confirm-body.json";
import createPaymentBody from "../../fixtures/create-payment-body.json";
import State from "../../utils/State";
import getConnectorDetails from "../ConnectorUtils/utils";

let globalState;

describe("Card - NoThreeDS payment flow test", () => {

  before("seed global state", () => {

    cy.task('getGlobalState').then((state) => {
      globalState = new State(state);
      console.log("seeding globalState -> " + JSON.stringify(globalState));
    })
  })

  after("flush global state", () => {
    console.log("flushing globalState -> " + JSON.stringify(globalState));
    cy.task('setGlobalState', globalState.data);
  })

  context("Card-NoThreeDS payment flow test Create and confirm", () => {

    it("create-payment-call-test", () => {
      let det = getConnectorDetails(globalState.get("connectorId"))["card_pm"]["No3DS"];
      cy.createPaymentIntentTest(createPaymentBody, det, "no_three_ds", "automatic", globalState);
    });

    it("payment_methods-call-test", () => {
      cy.paymentMethodsCallTest(globalState);
    });

    it("Confirm No 3DS", () => {
      let det = getConnectorDetails(globalState.get("connectorId"))["card_pm"]["No3DS"];
      cy.confirmCallTest(confirmBody, det, true, globalState);
    });

    it("retrieve-payment-call-test", () => {
      cy.retrievePaymentCallTest(globalState);
    });

  });

  context("Card-NoThreeDS payment flow test Create+Confirm", () => {

    it("create+confirm-payment-call-test", () => {
      console.log("confirm -> " + globalState.get("connectorId"));
      let det = getConnectorDetails(globalState.get("connectorId"))["card_pm"]["No3DS"];
      cy.createConfirmPaymentTest(createConfirmPaymentBody, det, "no_three_ds", "automatic", globalState);
    });

    it("retrieve-payment-call-test", () => {
      cy.retrievePaymentCallTest(globalState);
    });


  });
});