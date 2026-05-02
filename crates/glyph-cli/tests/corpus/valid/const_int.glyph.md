skill retry_task()
    description: "Retry a task with a fixed budget."
    require max_attempts
    flow:
        "Attempt the task."

const max_attempts = 3
