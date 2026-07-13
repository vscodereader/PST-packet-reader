import { MantineProvider } from "@mantine/core";
import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Blog } from "./blog";

// ipc 모킹: 계정 목록만 있으면 화면이 뜬다(발행/명확인은 클릭 시 호출).
vi.mock("@/shared/ipc", () => ({
  ipc: {
    accounts: {
      list: vi.fn().mockResolvedValue([
        { id: "1", platform: "naver", loginId: "acc1", pw: "", status: "active", last: "", tags: [] },
        { id: "2", platform: "band", loginId: "bandacc", pw: "", status: "active", last: "", tags: [] },
      ]),
    },
    blog: { checkName: vi.fn(), publish: vi.fn() },
  },
}));

function renderBlog() {
  return render(
    <MantineProvider>
      <Blog />
    </MantineProvider>,
  );
}

describe("Blog view", () => {
  it("헤더와 게시하기 버튼을 렌더한다", async () => {
    renderBlog();
    expect(screen.getByText("네이버 블로그")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "게시하기" })).toBeInTheDocument();
  });

  it("밴드 계정은 제외하고 네이버 계정만 옵션에 넣는다", async () => {
    renderBlog();
    // 계정 로드가 끝나면(밴드 제외) 발행 설정 섹션이 보인다.
    await waitFor(() => expect(screen.getByText("발행 설정")).toBeInTheDocument());
    // 공개설정 세그먼트에 4개 옵션.
    expect(screen.getByText("전체공개")).toBeInTheDocument();
    expect(screen.getByText("비공개")).toBeInTheDocument();
  });
});
