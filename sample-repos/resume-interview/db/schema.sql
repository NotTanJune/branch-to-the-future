create table resumes (
  id text primary key,
  object_key text not null,
  candidate_name text,
  parsed_json jsonb,
  created_at timestamp default now()
);
