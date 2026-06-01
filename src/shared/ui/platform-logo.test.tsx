import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";

import bandLogo from "@/assets/logos/band.svg";
import naverLogo from "@/assets/logos/naver.svg";
import navercafeLogo from "@/assets/logos/navercafe.svg";
import threadsLogo from "@/assets/logos/threads.svg";

import { PlatformLogo } from "./platform-logo";

function renderLogo(props: Parameters<typeof PlatformLogo>[0]) {
  render(
    <MantineProvider>
      <PlatformLogo {...props} />
    </MantineProvider>,
  );
}

describe("PlatformLogo", () => {
  it("renders the real brand image for a platform that has a logo", () => {
    renderLogo({ id: "band" });
    const img = screen.getByRole("img", { name: "밴드" });
    expect(img).toHaveAttribute("src", bandLogo);
  });

  it("uses the naver cafe logo, not the plain naver mark, for the naver platform", () => {
    renderLogo({ id: "naver" });
    const img = screen.getByRole("img", { name: "네이버 카페" });
    expect(img).toHaveAttribute("src", navercafeLogo);
    expect(img.getAttribute("src")).not.toBe(naverLogo);
  });

  it("renders the brand image for the forum platform", () => {
    renderLogo({ id: "forum" });
    const img = screen.getByRole("img", { name: "종합토론방" });
    expect(img).toHaveAttribute("src", naverLogo);
  });

  it("renders the threads brand image", () => {
    renderLogo({ id: "threads" });
    expect(screen.getByRole("img", { name: "스레드" })).toHaveAttribute(
      "src",
      threadsLogo,
    );
  });

  it("dims the logo when dim is set", () => {
    renderLogo({ id: "threads", dim: true });
    const img = screen.getByRole("img", { name: "스레드" });
    expect(img).toHaveStyle({ opacity: "0.5" });
  });
});
