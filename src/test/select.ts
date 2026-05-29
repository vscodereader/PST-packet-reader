import userEvent from "@testing-library/user-event";

/**
 * Open the Nth Mantine `Select` on screen and click the option whose text
 * matches `label`. jsdom doesn't classify Mantine's portalled options under
 * the listbox role, so we locate them by the `role="option"` attribute.
 */
export async function pickOption(index: number, label: string) {
  const combos = document.querySelectorAll<HTMLInputElement>(
    'input[aria-haspopup="listbox"]',
  );
  await userEvent.click(combos[index]!);
  const opt = [...document.querySelectorAll('[role="option"]')].find(
    (o) => o.textContent === label,
  );
  await userEvent.click(opt as Element);
}
