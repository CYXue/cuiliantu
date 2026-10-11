import { open } from "@tauri-apps/plugin-dialog";

// Shared output-directory picker (convert & upload-prep panels).
export async function pickOutputDir(title?: string): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title,
  });
  return typeof selected === "string" ? selected : null;
}
