skill deploy_app()
    description: "Deploy an application."
    flow:
        context deployment_rules
        context "This is a production deployment."
        "Build the application."
        "Run tests."
        "Deploy to production."

text deployment_rules = "Follow the deployment checklist before deploying."
