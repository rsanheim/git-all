#!/usr/bin/env ruby
# frozen_string_literal: true

require_relative "lib/nit/repo"
require_relative "lib/nit/runner"
require_relative "lib/nit/commands/status"
require_relative "lib/nit/commands/pull"
require_relative "lib/nit/commands/fetch"
require_relative "lib/nit/commands/passthrough"
require_relative "lib/nit/cli"

Nit::CLI.new.run
