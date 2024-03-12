// Validate status 2xx 
pm.test("[POST]::/routing/payouts/deactivate - Status code is 2xx", function () {
   pm.response.to.be.success;
});

// Validate if response header has matching content-type
pm.test("[POST]::/routing/payouts/deactivate - Content-Type is application/json", function () {
   pm.expect(pm.response.headers.get("Content-Type")).to.include("application/json");
});

// Validate if response has JSON Body 
pm.test("[POST]::/routing/payouts/deactivate - Response has JSON Body", function () {
    pm.response.to.have.jsonBody();
});

// Set response object as internal variable
let jsonData = {};
try {
  jsonData = pm.response.json();
} catch(e) {
}

// Validate if algorithm kind is volume_split
pm.test("[POST]::/routing/payouts/deactivate - Algorithm configured for payouts", function () {
    pm.expect(jsonData.kind).to.eql("volume_split");
});

// Validate if algorithm was configured for payouts
pm.test("[POST]::/routing/payouts/deactivate - Algorithm configured for payouts", function () {
    pm.expect(jsonData.algorithm_for).to.eql("payout");
});