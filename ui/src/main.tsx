import { render } from "preact";
import { App } from "./App";
import { SourceProvider } from "./source";
import "./style.css";

render(
  <SourceProvider>
    <App />
  </SourceProvider>,
  document.getElementById("app")!,
);
