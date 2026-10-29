import { render } from "preact";
import { App } from "./App";
import { SourceProvider } from "./source";
import { Tooltip } from "./Tooltip";
import "./style.css";

render(
  <SourceProvider>
    <App />
    <Tooltip />
  </SourceProvider>,
  document.getElementById("app")!,
);
