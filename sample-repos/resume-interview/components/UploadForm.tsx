"use client";

import { useState } from "react";

export function UploadForm() {
  const [summary, setSummary] = useState<string>("");

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const response = await fetch("/api/upload", {
      method: "POST",
      body: form
    });
    const result = await response.json();
    setSummary(`${result.candidate} has ${result.skills.length} detected skills.`);
  }

  return (
    <form onSubmit={handleSubmit}>
      <input name="resume" type="file" accept=".pdf,.doc,.docx" />
      <button type="submit">Upload resume</button>
      <p>{summary}</p>
    </form>
  );
}
