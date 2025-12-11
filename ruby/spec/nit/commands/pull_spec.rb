# frozen_string_literal: true

require "spec_helper"

RSpec.describe Nit::Commands::Pull do
  let(:formatter) { described_class.new }

  def output(stdout: "", stderr: "", success: true)
    Nit::Output.new(stdout:, stderr:, success:)
  end

  describe "#format" do
    context "when already up to date" do
      it 'returns "Already up to date"' do
        result = formatter.format(output(stdout: "Already up to date.\n"))
        expect(result).to eq("Already up to date")
      end
    end

    context "with file changes" do
      it "extracts files changed summary" do
        stdout = <<~OUT
          Updating abc123..def456
          Fast-forward
           file.txt | 10 +++++++---
           1 file changed, 7 insertions(+), 3 deletions(-)
        OUT
        result = formatter.format(output(stdout:))
        expect(result).to eq("1 file changed, 7 insertions(+), 3 deletions(-)")
      end
    end

    context "with fast-forward info" do
      it "extracts Updating line" do
        stdout = "Updating abc123..def456\n"
        result = formatter.format(output(stdout:))
        expect(result).to eq("Updating abc123..def456")
      end
    end

    context "fallback behavior" do
      it "returns first non-empty line" do
        result = formatter.format(output(stdout: "\nSome output\n"))
        expect(result).to eq("Some output")
      end

      it 'returns "completed" when output is empty' do
        result = formatter.format(output(stdout: "", stderr: ""))
        expect(result).to eq("completed")
      end
    end

    context "on error" do
      it "returns error message from stderr" do
        result = formatter.format(output(
          stderr: "error: cannot pull with rebase\n",
          success: false
        ))
        expect(result).to eq("ERROR: error: cannot pull with rebase")
      end
    end
  end

  describe "#build_command" do
    it "creates pull command" do
      cmd = formatter.build_command("/path/to/repo")
      expect(cmd.to_s).to eq("git -C /path/to/repo pull")
    end

    it "includes extra args" do
      cmd = formatter.build_command("/path/to/repo", ["--all", "--verbose"])
      expect(cmd.to_s).to eq("git -C /path/to/repo pull --all --verbose")
    end
  end
end
