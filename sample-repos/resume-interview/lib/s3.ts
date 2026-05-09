import { S3Client, PutObjectCommand } from "@aws-sdk/client-s3";

const client = new S3Client({ region: process.env.AWS_REGION });

export async function saveResumeObject(file: File) {
  const key = `resumes/${crypto.randomUUID()}-${file.name}`;
  const bytes = Buffer.from(await file.arrayBuffer());

  await client.send(new PutObjectCommand({
    Bucket: process.env.RESUME_BUCKET,
    Key: key,
    Body: bytes,
    ContentType: file.type
  }));

  return key;
}
