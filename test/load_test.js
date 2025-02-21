import http from "k6/http";

export let options = {
  stages: [
    { duration: "30s", target: 1000 },  // Ramp up to 1,000 users
    { duration: "1m", target: 5000 },   // Ramp up to 5,000 users
    { duration: "2m", target: 10000 },  // Ramp up to 10,000 users
    { duration: "1m", target: 10000 },  // Hold at 10,000 users
    { duration: "30s", target: 0 },     // Ramp down
  ],
};

export default function() {
  http.get("http://localhost:8080/poster?movie=Avengers");
}
