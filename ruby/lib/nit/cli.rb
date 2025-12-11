# frozen_string_literal: true

require "optparse"

module Nit
  VERSION = "0.1.0"

  class CLI
    attr_reader :workers, :dry_run, :command, :args

    def initialize(argv = ARGV.dup)
      @workers = 8
      @dry_run = false
      @command = nil
      @args = []
      parse(argv.dup)
    end

    def run
      repos = Repo.find_git_repos
      if repos.empty?
        puts "No git repositories found in current directory"
        return
      end

      if dry_run
        puts "[nit v#{VERSION}] Running in **dry-run mode**, no git commands will be executed. Planned git commands below."
      end

      runner = Runner.new(workers:, dry_run:)
      formatter = formatter_for_command

      build_command = ->(repo_path) { formatter.build_command(repo_path, args) }
      runner.run_parallel(repos, build_command:, formatter:)
    end

    private

    def parse(argv)
      # Extract nit-specific options from anywhere in argv
      # This allows: nit status --dry-run OR nit --dry-run status
      nit_opts = []
      git_args = []
      command_found = false

      i = 0
      while i < argv.length
        arg = argv[i]

        case arg
        when "-h", "--help"
          show_help
          exit
        when "-V", "--version"
          puts "nit v#{VERSION}"
          exit
        when "--dry-run"
          @dry_run = true
        when "-n", "--workers"
          i += 1
          @workers = argv[i].to_i
        when /^--workers=(\d+)$/
          @workers = ::Regexp.last_match(1).to_i
        when /^-n(\d+)$/
          @workers = ::Regexp.last_match(1).to_i
        else
          if command_found
            git_args << arg
          else
            @command = arg
            command_found = true
          end
        end
        i += 1
      end

      @args = git_args

      if @command.nil?
        puts "No command specified. Use --help for usage information."
        exit 1
      end
    end

    def show_help
      puts <<~HELP
        Usage: nit [options] <command> [git-args...]

        Commands:
          pull             Pull all repositories
          fetch            Fetch all repositories
          status           Status all repositories
          [anything else]  Pass through to git

        Global options:
            -n, --workers N              Number of parallel workers (default: 8)
                --dry-run                Print exact commands without executing
            -h, --help                   Show this help message
            -V, --version                Show version
      HELP
    end

    def formatter_for_command
      case command
      when "pull"
        Commands::Pull.new
      when "fetch"
        Commands::Fetch.new
      when "status"
        Commands::Status.new
      else
        # Passthrough - include command in args
        @args = [command] + args
        Commands::Passthrough.new
      end
    end
  end
end
