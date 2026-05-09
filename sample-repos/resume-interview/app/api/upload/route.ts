import { saveResumeObject } from "@/lib/s3";
import { parseResumeSync } from "@/lib/parser";

export async function POST(request: Request) {
  const data = await request.formData();
  const file = data.get("resume") as File;
  const objectKey = await saveResumeObject(file);
  const parsed = await parseResumeSync(file);

  return Response.json({
    objectKey,
    candidate: parsed.candidate,
    skills: parsed.skills
  });
}
