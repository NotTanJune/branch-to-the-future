# Branch to the Future Report

## Change Request

add async resume parsing with S3 upload and status polling

## Repo Summary

- Repo: resume-interview
- Summary: Most impact centers on app/api/upload/route.ts, workers/parser.ts, lib/s3.ts, lib/parser.ts, components/UploadForm.tsx, and tests/upload.test.ts. A new status endpoint and likely durable job/status storage are needed for reliable polling.

## Impact Path

1. `app/api/upload/route.ts` (1/10, confidence 95%, risk high)
   - Reason: Current upload route imports saveResumeObject and parseResumeSync, so it is the main sync bottleneck and API contract point.
   - Change: Replace synchronous parse-on-upload with S3 save plus pending job response; add validation and job identifier handling.
2. `lib/s3.ts` (1/10, confidence 85%, risk medium)
   - Reason: Existing S3 library only shows PutObject usage; worker needs to read uploaded resume or otherwise access contents later.
   - Change: Add object retrieval helper or metadata support so async worker can parse files stored in S3.
3. `lib/parser.ts` (1/10, confidence 80%, risk medium)
   - Reason: Current parseResumeSync accepts File and reads file.text(); async worker parsing from S3 likely has Buffer/string, not browser File.
   - Change: Adapt parsing function for worker usage, possibly accepting bytes/text instead of File only.
4. `workers/parser.ts` (1/10, confidence 90%, risk medium)
   - Reason: Existing worker is a stub importing parser and accepting objectKey; proposed change directly targets this file.
   - Change: Implement actual async parse flow using objectKey/jobId and persist or expose status updates.
5. `components/UploadForm.tsx` (1/10, confidence 90%, risk medium)
   - Reason: Current form likely expects immediate parsed result from /api/upload; async polling changes UX and response handling.
   - Change: Update client flow to handle upload accepted, poll status endpoint, display processing/completion/error states.
6. `components/FeedbackPanel.tsx` (0/10, confidence 45%, risk low)
   - Reason: Feedback panel may need parsed resume data or status, but current summary does not show props/state integration.
   - Change: May need to consume async parsed resume or feedback trigger after parsing completes.
7. `app/page.tsx` (1/10, confidence 50%, risk medium)
   - Reason: Page composes UploadForm and FeedbackPanel; async status/result may require lifted state if feedback depends on parsing.
   - Change: May need to pass parsed result or selected job id between UploadForm and FeedbackPanel.
8. `db/schema.sql` (1/10, confidence 70%, risk medium)
   - Reason: Polling requires durable status storage unless implemented in memory; schema already exists but DB access is not visible.
   - Change: Potentially add status/result persistence fields or job table.
9. `tests/upload.test.ts` (1/10, confidence 95%, risk medium)
   - Reason: Only existing test file is upload-focused and current risk signal notes upload validation.
   - Change: Update and expand tests for async upload contract, polling, validation, and mocked S3/parser behavior.
10. `package.json` (0/10, confidence 40%, risk low)
   - Reason: Queue, DB client, or test utilities may require dependencies, but no specific implementation is known.
   - Change: Possible dependency or script additions for worker/queue/testing mocks if a real async backend is introduced.
11. `app/api/feedback/route.ts` (0/10, confidence 35%, risk low)
   - Reason: Feedback route is separate and OpenAI-backed; only impacted if async parse result feeds feedback workflow.
   - Change: No direct change expected unless feedback generation is triggered automatically after parsing.
12. `lib/openai.ts` (0/10, confidence 30%, risk low)
   - Reason: OpenAI module creates interview feedback and is not part of upload/parse status path.
   - Change: No direct change expected unless parsed resume format changes for feedback prompts.

## Affected Files

- `app/api/upload/route.ts`
- `lib/s3.ts`
- `lib/parser.ts`
- `workers/parser.ts`
- `components/UploadForm.tsx`
- `components/FeedbackPanel.tsx`
- `app/page.tsx`
- `db/schema.sql`
- `tests/upload.test.ts`
- `package.json`
- `app/api/feedback/route.ts`
- `lib/openai.ts`

## Risk Summary

- Main contract break: /api/upload likely changes from returning parsed data to returning pending job id/status.
- Polling requires status storage; in-memory status is risky in Next.js/serverless deployments.
- Worker currently appears stubbed and may lack a real trigger/queue mechanism.
- Parser currently accepts File, which may not match S3 object data in a worker.
- File upload validation is already flagged and should be addressed before expanding upload flow.
- S3 and OpenAI environment-dependent behavior should be mocked in tests.

## Branch to the Future

### Async S3-backed parsing with polling

- Complexity: medium
- Risk: medium
- Description: Upload file to S3, enqueue or trigger async parsing, return an object key/job id, and let UI poll parsing status.
- Affected files:
  - `app/api/upload/route.ts`
  - `lib/s3.ts`
  - `lib/parser.ts`
  - `workers/parser.ts`
  - `components/UploadForm.tsx`
  - `tests/upload.test.ts`
- Benefits:
  - Keeps existing simple API shape mostly intact
  - Adds async behavior with moderate code churn
  - Improves upload latency by returning before parsing completes
- Drawbacks:
  - Requires persistent status tracking not clearly present in current repo
  - More states in UI: uploading, processing, complete, failed
  - Worker may need real queue/invocation mechanism beyond current stub

### In-process async parsing and polling

- Complexity: low
- Risk: high
- Description: Make upload endpoint return pending status while parsing happens via in-memory background task or delayed promise.
- Affected files:
  - `app/api/upload/route.ts`
  - `lib/parser.ts`
  - `components/UploadForm.tsx`
  - `tests/upload.test.ts`
- Benefits:
  - Lowest infrastructure complexity
  - Can simulate async behavior in-process for a small app
  - Minimal S3 changes
- Drawbacks:
  - Not durable across server restarts or serverless invocations
  - Not reliable in Next.js production/serverless environments
  - Status storage may be lost

### Durable DB-backed async pipeline

- Complexity: high
- Risk: medium
- Description: Introduce durable job table/status persistence and worker-driven parsing from S3.
- Affected files:
  - `app/api/upload/route.ts`
  - `db/schema.sql`
  - `workers/parser.ts`
  - `lib/s3.ts`
  - `components/UploadForm.tsx`
  - `tests/upload.test.ts`
- Benefits:
  - Durable job status and parsed results
  - Best fit for real async processing
  - Clear separation of upload, processing, and status reads
- Drawbacks:
  - No database client file is visible, so persistence layer must be added
  - Requires migrations and operational queue/worker decisions
  - Larger blast radius and more integration tests


## Recommended Path

Async S3-backed parsing with polling

Selected export path: Async S3-backed parsing with polling

## Test Plan

- Unit test upload rejects missing/invalid files
- Unit test upload returns pending job id without parsed resume
- Unit test status endpoint returns pending/complete/error
- Mock S3 and parser in route tests
- Test UploadForm polling success and failure states

## Patch Skeleton

### `app/api/upload/route.ts`

- Change /api/upload to validate file, save to S3, create processing status, return jobId/objectKey immediately
- Update workers/parser.ts to read uploaded object, parse resume, and persist completion/error status
- Add a status route, likely proposed new file app/api/upload/status/route.ts or app/api/resume/[id]/route.ts
- Extend lib/s3.ts with get/read helper if worker parses from S3
- Adjust UploadForm to poll status after upload and render progress/errors
- Update tests for accepted upload response and polling lifecycle

### `lib/s3.ts`

- Change /api/upload to validate file, save to S3, create processing status, return jobId/objectKey immediately
- Update workers/parser.ts to read uploaded object, parse resume, and persist completion/error status
- Add a status route, likely proposed new file app/api/upload/status/route.ts or app/api/resume/[id]/route.ts
- Extend lib/s3.ts with get/read helper if worker parses from S3
- Adjust UploadForm to poll status after upload and render progress/errors
- Update tests for accepted upload response and polling lifecycle

### `lib/parser.ts`

- Change /api/upload to validate file, save to S3, create processing status, return jobId/objectKey immediately
- Update workers/parser.ts to read uploaded object, parse resume, and persist completion/error status
- Add a status route, likely proposed new file app/api/upload/status/route.ts or app/api/resume/[id]/route.ts
- Extend lib/s3.ts with get/read helper if worker parses from S3
- Adjust UploadForm to poll status after upload and render progress/errors
- Update tests for accepted upload response and polling lifecycle

### `workers/parser.ts`

- Change /api/upload to validate file, save to S3, create processing status, return jobId/objectKey immediately
- Update workers/parser.ts to read uploaded object, parse resume, and persist completion/error status
- Add a status route, likely proposed new file app/api/upload/status/route.ts or app/api/resume/[id]/route.ts
- Extend lib/s3.ts with get/read helper if worker parses from S3
- Adjust UploadForm to poll status after upload and render progress/errors
- Update tests for accepted upload response and polling lifecycle

### `components/UploadForm.tsx`

- Change /api/upload to validate file, save to S3, create processing status, return jobId/objectKey immediately
- Update workers/parser.ts to read uploaded object, parse resume, and persist completion/error status
- Add a status route, likely proposed new file app/api/upload/status/route.ts or app/api/resume/[id]/route.ts
- Extend lib/s3.ts with get/read helper if worker parses from S3
- Adjust UploadForm to poll status after upload and render progress/errors
- Update tests for accepted upload response and polling lifecycle

### `tests/upload.test.ts`

- Change /api/upload to validate file, save to S3, create processing status, return jobId/objectKey immediately
- Update workers/parser.ts to read uploaded object, parse resume, and persist completion/error status
- Add a status route, likely proposed new file app/api/upload/status/route.ts or app/api/resume/[id]/route.ts
- Extend lib/s3.ts with get/read helper if worker parses from S3
- Adjust UploadForm to poll status after upload and render progress/errors
- Update tests for accepted upload response and polling lifecycle


## Image Artifact

Architecture diagram: `/Users/nottanjune/Code-Projects/branch-to-the-future/sample-repos/resume-interview/branch-to-the-future-architecture.png`
