class State {
  data = {};
  constructor(data) {
    this.data = data;
    this.data["connectorId"] = Cypress.env("CONNECTOR");
    this.data["baseUrl"] = Cypress.env("BASEURL");
    this.data["adminApiKey"] = Cypress.env("ADMINAPIKEY");
    this.data["email"] = Cypress.env("HS_EMAIL");
    this.data["password"] = Cypress.env("HS_PASSWORD");
    this.data["connectorAuthFilePath"] = Cypress.env(
      "CONNECTOR_AUTH_FILE_PATH"
    );
    this.data["apiKey"] = Cypress.env("API_KEY");
    this.data["publishableKey"] = Cypress.env("PUBLISHABLE_KEY");
  }

  set(key, val) {
    this.data[key] = val;
  }

  get(key) {
    return this.data[key];
  }
}

export default State;
