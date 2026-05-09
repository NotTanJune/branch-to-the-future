import { createInterviewFeedback } from "@/lib/openai";

export async function POST(request: Request) {
  const body = await request.json();
  const feedback = await createInterviewFeedback(body.resumeText, body.role);

  return Response.json({ feedback });
}
