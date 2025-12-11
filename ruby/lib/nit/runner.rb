# frozen_string_literal: true

require "open3"

module Nit
  # Result of capturing git command output
  Output = Data.define(:stdout, :stderr, :success)

  # Result of executing a git command
  module CommandResult
    DryRun = Data.define(:command_string)
    Executed = Data.define(:repo_name, :output)
    Error = Data.define(:repo_name, :message)
  end

  # A git command ready to be executed against a repository
  class GitCommand
    attr_reader :repo_path, :command, :args

    def initialize(repo_path, command, args = [])
      @repo_path = repo_path
      @command = command
      @args = args
    end

    # Build the command string for display (used in dry-run and errors)
    def to_s
      args_str = args.empty? ? "" : " #{args.join(" ")}"
      "git -C #{repo_path} #{command}#{args_str}"
    end

    # Execute the git command
    def execute
      stdout, stderr, status = Open3.capture3("git", "-C", repo_path, command, *args)
      Output.new(stdout:, stderr:, success: status.success?)
    end
  end

  # Run commands in parallel across all repos
  class Runner
    attr_reader :workers, :dry_run

    def initialize(workers: 8, dry_run: false)
      @workers = workers
      @dry_run = dry_run
      @output_mutex = Mutex.new
    end

    # Run git commands across repos with the given formatter
    # build_command: ->(repo_path) { GitCommand.new(...) }
    # formatter: responds to #format(output) -> String
    def run_parallel(repos, build_command:, formatter:)
      # Use stdlib Thread pool instead of parallel gem for faster startup
      queue = Queue.new
      repos.each { |r| queue << r }

      threads = workers.times.map do
        Thread.new do
          while (repo_path = queue.pop(true) rescue nil)
            cmd = build_command.call(repo_path)
            result = execute_command(cmd)
            output_line = format_result(result, formatter)
            print_line(output_line)
          end
        end
      end

      threads.each(&:join)
    end

    private

    def execute_command(cmd)
      # Single code path: build the command string first (for dry-run accuracy)
      cmd_string = cmd.to_s

      if dry_run
        CommandResult::DryRun.new(command_string: cmd_string)
      else
        begin
          output = cmd.execute
          repo_name = Repo.repo_name(cmd.repo_path)
          CommandResult::Executed.new(repo_name:, output:)
        rescue StandardError => e
          repo_name = Repo.repo_name(cmd.repo_path)
          CommandResult::Error.new(repo_name:, message: e.message)
        end
      end
    end

    def format_result(result, formatter)
      case result
      in CommandResult::DryRun(command_string:)
        command_string
      in CommandResult::Executed(repo_name:, output:)
        formatted = formatter.format(output)
        "#{Repo.format_repo_name(repo_name)} #{formatted}"
      in CommandResult::Error(repo_name:, message:)
        "#{Repo.format_repo_name(repo_name)} ERROR: #{message}"
      end
    end

    def print_line(line)
      @output_mutex.synchronize do
        puts line
      end
    end
  end
end
