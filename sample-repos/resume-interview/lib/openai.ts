import OpenAI from "openai";

const client = new OpenAI({ apiKey: process.env.OPENAI_API_KEY });

export async function createInterviewFeedback(resumeText: string, role: string) {
  const response = await client.responses.create({
    model: "gpt-5-mini",
    input: `Create interview feedback for ${role}: ${resumeText}`
  });

  return response.output_text;
}
