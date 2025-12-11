# frozen_string_literal: true

require "fileutils"
require "tmpdir"

require_relative "../lib/nit/repo"
require_relative "../lib/nit/runner"
require_relative "../lib/nit/commands/status"
require_relative "../lib/nit/commands/pull"
require_relative "../lib/nit/commands/fetch"
require_relative "../lib/nit/commands/passthrough"

RSpec.configure do |config|
  config.expect_with :rspec do |expectations|
    expectations.include_chain_clauses_in_custom_matcher_descriptions = true
  end

  config.mock_with :rspec do |mocks|
    mocks.verify_partial_doubles = true
  end

  config.shared_context_metadata_behavior = :apply_to_host_groups
  config.filter_run_when_matching :focus
  config.disable_monkey_patching!
  config.warnings = true
  config.order = :random
  Kernel.srand config.seed
end
