export async function parseResumeSync(file: File) {
  const text = await file.text();
  const skills = ["TypeScript", "React", "SQL"].filter((skill) => text.includes(skill));

  return {
    candidate: file.name.replace(/\..+$/, ""),
    skills
  };
}
