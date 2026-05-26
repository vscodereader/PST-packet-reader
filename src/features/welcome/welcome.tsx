import { useMutation } from "@tanstack/react-query";
import { useState } from "react";

import reactLogo from "@/assets/react.svg";
import { commands } from "@/shared/bindings/commands";

import "./welcome.css";

export function Welcome() {
  const [name, setName] = useState("");
  const greetMutation = useMutation({
    mutationFn: (input: string) => commands.greet(input),
  });

  return (
    <main className="container">
      <h1>Welcome to Tauri + React</h1>

      <div className="row">
        <a href="https://vite.dev" target="_blank" rel="noreferrer">
          <img src="/vite.svg" className="logo vite" alt="Vite logo" />
        </a>
        <a href="https://tauri.app" target="_blank" rel="noreferrer">
          <img src="/tauri.svg" className="logo tauri" alt="Tauri logo" />
        </a>
        <a href="https://react.dev" target="_blank" rel="noreferrer">
          <img src={reactLogo} className="logo react" alt="React logo" />
        </a>
      </div>
      <p>Click on the Tauri, Vite, and React logos to learn more.</p>

      <form
        className="row"
        onSubmit={(e) => {
          e.preventDefault();
          greetMutation.mutate(name);
        }}
      >
        <input
          id="greet-input"
          onChange={(e) => setName(e.currentTarget.value)}
          placeholder="Enter a name..."
        />
        <button type="submit" disabled={greetMutation.isPending}>
          {greetMutation.isPending ? "Greeting..." : "Greet"}
        </button>
      </form>
      {greetMutation.data && <p>{greetMutation.data}</p>}
      {greetMutation.error && (
        <p role="alert">Error: {greetMutation.error.message}</p>
      )}
    </main>
  );
}
