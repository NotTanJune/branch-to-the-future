import { parseResumeSync } from "@/lib/parser";

export async function parseUploadedResume(objectKey: string) {
  return {
    objectKey,
    status: "completed",
    result: await parseResumeSync(new File([], objectKey))
  };
}
