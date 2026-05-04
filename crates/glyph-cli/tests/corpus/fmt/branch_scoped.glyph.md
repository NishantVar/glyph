skill deploy()
    description: "Deploy the application."
    flow:
        "Prepare deployment."
        if env == "production"
            require safety_checks
            context production_config
            "Deploy to production."
        else
            "Deploy to staging."

const safety_checks = "run all safety checks"
const production_config = "Production uses strict settings."
