import { SessionProvider } from "contexts/session";

import Router from "./Router";

import "./styles/index.css";

export default function App() {
  return (
    <SessionProvider fallback={null}>
      <Router />
    </SessionProvider>
  );
}
