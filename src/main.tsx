import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import "./ds/tokens.css";
import "./ds/ds.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
