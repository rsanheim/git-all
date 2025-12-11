# frozen_string_literal: true

module Nit
  module Commands
    class Passthrough
      # Format passthrough output - just show first non-empty line or "ok"
      def format(output)
        return error_line(output.stderr) unless output.success

        all_lines = output.stdout.lines + output.stderr.lines
        first_line = all_lines.find { |l| !l.strip.empty? }
        first_line&.strip || "ok"
      end

      # Build git command for passthrough (command and args passed as-is)
      def build_command(repo_path, args)
        raise ArgumentError, "No git command specified" if args.empty?

        command = args.first
        extra_args = args[1..] || []
        GitCommand.new(repo_path, command, extra_args)
      end

      private

      def error_line(stderr)
        line = stderr.lines.find { |l| !l.strip.empty? } || "unknown error"
        "ERROR: #{line.strip}"
      end
    end
  end
end
