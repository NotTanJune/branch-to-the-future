import { UploadForm } from "@/components/UploadForm";
import { FeedbackPanel } from "@/components/FeedbackPanel";

export default function Page() {
  return (
    <main>
      <h1>Resume Interview</h1>
      <UploadForm />
      <FeedbackPanel />
    </main>
  );
}
