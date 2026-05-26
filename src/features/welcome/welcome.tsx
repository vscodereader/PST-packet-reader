import {
  Anchor,
  Button,
  Container,
  Group,
  Image,
  Stack,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";

import reactLogo from "@/assets/react.svg";

export function Welcome() {
  const [greetMsg, setGreetMsg] = useState("");
  const [name, setName] = useState("");

  async function greet() {
    setGreetMsg(await invoke("greet", { name }));
  }

  return (
    <Container size="sm" py="xl">
      <Stack align="center" gap="md">
        <Title order={1}>Welcome to Tauri + React</Title>

        <Group justify="center" gap="lg">
          <Anchor href="https://vite.dev" target="_blank" rel="noreferrer">
            <Image
              src="/vite.svg"
              alt="Vite logo"
              h={64}
              w={64}
              fit="contain"
            />
          </Anchor>
          <Anchor href="https://tauri.app" target="_blank" rel="noreferrer">
            <Image
              src="/tauri.svg"
              alt="Tauri logo"
              h={64}
              w={64}
              fit="contain"
            />
          </Anchor>
          <Anchor href="https://react.dev" target="_blank" rel="noreferrer">
            <Image
              src={reactLogo}
              alt="React logo"
              h={64}
              w={64}
              fit="contain"
            />
          </Anchor>
        </Group>
        <Text c="dimmed">
          Click on the Tauri, Vite, and React logos to learn more.
        </Text>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            greet();
          }}
        >
          <Group>
            <TextInput
              id="greet-input"
              value={name}
              onChange={(e) => setName(e.currentTarget.value)}
              placeholder="Enter a name..."
            />
            <Button type="submit">Greet</Button>
          </Group>
        </form>

        {greetMsg && <Text>{greetMsg}</Text>}
      </Stack>
    </Container>
  );
}
