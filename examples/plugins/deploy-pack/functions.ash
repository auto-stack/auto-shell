// functions.ash — contributed AutoLang functions (sourced on plugin load).
// Defines a helper callable from the prompt: deploy_msg("prod").

fn deploy_msg(env) {
    return "deploy-pack: preparing " + env + " release"
}
