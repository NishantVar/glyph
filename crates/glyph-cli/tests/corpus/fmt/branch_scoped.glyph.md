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

text safety_checks = "Run all safety checks."
text production_config = "Production uses strict settings."
