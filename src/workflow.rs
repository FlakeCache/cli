use anyhow::Result;
use std::fs;
use std::path::Path;

pub fn generate_workflow_template() -> String {
    r"name: FlakeCache CI

on:
  push:
    branches: [ main, development ]
  pull_request:
    branches: [ main ]
  workflow_dispatch:

jobs:
  build:
    name: Build and Test
    runs-on: ubuntu-latest
    
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Setup Nix
        uses: DeterminateSystems/nix-installer-action@main

      - name: Build
        run: nix build

      - name: Test
        run: nix flake check

  publish:
    name: Publish to FlakeCache
    runs-on: ubuntu-latest
    needs: build
    if: github.ref == 'refs/heads/main'
    
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Setup Nix and FlakeCache
        uses: flakecache/cache@v1
        with:
          cache-name: ${{ github.repository }}
          token: ${{ secrets.FLAKECACHE_TOKEN }}
          # This action configures Nix to download from FlakeCache and publishes new builds
          # Get your token from: https://flakecache.com/settings/tokens
"
    .to_string()
}

#[allow(clippy::unused_async)] // Async signature for API consistency with generate_script
pub async fn generate_workflow(output_path: &str) -> Result<()> {
    let path = Path::new(output_path);

    // Create directory if it doesn't exist
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let workflow_content = generate_workflow_template();
    fs::write(path, workflow_content)?;

    println!("Generated workflow file: {output_path}");
    println!("\nNext steps:");
    println!("  1. Review and customize the workflow file");
    println!("  2. Get your FlakeCache API token from: https://flakecache.com/settings/tokens");
    println!(
        "  3. Set FLAKECACHE_TOKEN secret in repository settings (Settings → Secrets → Actions)"
    );
    println!("  4. Commit and push the workflow file to your repository");
    println!("\nNote: FLAKECACHE_TOKEN is your FlakeCache API token, NOT your GitHub token!");

    Ok(())
}

pub async fn generate_script(ci: &str, output: Option<&str>) -> Result<()> {
    let script_content = match ci.to_lowercase().as_str() {
        "jenkins" => generate_jenkins_script(),
        "gitlab" | "gitlab-ci" => generate_gitlab_script(),
        "circleci" | "circle" => generate_circleci_script(),
        "github" => {
            // For GitHub Actions, generate workflow
            let output_path = output.unwrap_or(".github/workflows/flakecache.yml");
            return generate_workflow(output_path).await;
        }
        "travis" | "travis-ci" => generate_travis_script(),
        "bitbucket" | "bitbucket-pipelines" => generate_bitbucket_script(),
        "buildkite" => generate_buildkite_script(),
        "tekton" => generate_tekton_script(),
        "drone" | "drone-ci" => generate_drone_script(),
        "azure-devops" | "azure" | "ado" => generate_azure_devops_script(),
        "aws-codebuild" | "codebuild" => generate_aws_codebuild_script(),
        "gcp-cloudbuild" | "cloudbuild" | "gcp" => generate_gcp_cloudbuild_script(),
        "argocd" | "argo" => generate_argocd_script(),
        "teamcity" => generate_teamcity_script(),
        "bamboo" => generate_bamboo_script(),
        "concourse" | "concourse-ci" => generate_concourse_script(),
        "spinnaker" => generate_spinnaker_script(),
        "generic" | "bash" => generate_generic_script(),
        _ => {
            return Err(anyhow::anyhow!(
                "Unknown CI system: {ci}. Supported: jenkins, gitlab, circleci, github, travis, bitbucket, buildkite, tekton, drone, azure-devops, aws-codebuild, gcp-cloudbuild, teamcity, bamboo, concourse, spinnaker, argocd, bash (or generic)"
            ));
        }
    };

    let default_path = match ci.to_lowercase().as_str() {
        "jenkins" => "Jenkinsfile.flakecache",
        "gitlab" | "gitlab-ci" => ".gitlab-ci-flakecache.yml",
        "circleci" | "circle" => ".circleci/flakecache-config.yml",
        "github" => ".github/workflows/flakecache.yml",
        "travis" | "travis-ci" => ".travis-flakecache.yml",
        "bitbucket" | "bitbucket-pipelines" => "bitbucket-pipelines-flakecache.yml",
        "buildkite" => ".buildkite/flakecache-pipeline.yml",
        "tekton" => "tekton/flakecache-pipeline.yaml",
        "drone" | "drone-ci" => ".drone-flakecache.yml",
        "azure-devops" | "azure" | "ado" => "azure-pipelines-flakecache.yml",
        "aws-codebuild" | "codebuild" => "buildspec-flakecache.yml",
        "gcp-cloudbuild" | "cloudbuild" | "gcp" => "cloudbuild-flakecache.yaml",
        "argocd" | "argo" => "argocd/flakecache-application.yaml",
        "teamcity" => "teamcity-flakecache-config.xml",
        "bamboo" => "bamboo-flakecache-specs.yaml",
        "concourse" | "concourse-ci" => "concourse/flakecache-pipeline.yml",
        "spinnaker" => "spinnaker/flakecache-pipeline.json",
        "generic" | "bash" => "flakecache-upload.sh",
        _ => "flakecache-script.sh",
    };

    let output_path = output.unwrap_or(default_path);

    let path = Path::new(&output_path);

    // Create directory if it doesn't exist
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, script_content)?;

    println!("Generated {ci} script: {output_path}");
    println!("\nNext steps:");
    println!("  1. Review and customize the script");
    println!("  2. Set FLAKECACHE_TOKEN environment variable in your CI");
    println!("  3. Set FLAKECACHE_CACHE to your cache name");
    println!("  4. Commit and use the script in your CI pipeline");

    Ok(())
}

fn generate_jenkins_script() -> String {
    r#"pipeline {
    agent any
    
    environment {
        FLAKECACHE_TOKEN = credentials('flakecache-token')
        FLAKECACHE_CACHE = 'my-cache'
    }
    
    stages {
        stage('Build') {
            steps {
                sh 'nix build'
            }
        }
        
        stage('Publish to FlakeCache') {
            steps {
                sh '''
                    export FLAKECACHE_TOKEN="${FLAKECACHE_TOKEN}"
                    export FLAKECACHE_CACHE="${FLAKECACHE_CACHE}"
                    bash scripts/flakecache-upload.sh
                '''
            }
        }
    }
}
"#
    .to_string()
}

fn generate_gitlab_script() -> String {
    r#"stages:
  - build
  - publish

build:
  stage: build
  script:
    - nix build
  artifacts:
    paths:
      - result

publish:
  stage: publish
  script:
    - |
      export FLAKECACHE_TOKEN="$FLAKECACHE_TOKEN"
      export FLAKECACHE_CACHE="${FLAKECACHE_CACHE:-my-cache}"
      bash scripts/flakecache-upload.sh
  only:
    - main
  variables:
    FLAKECACHE_CACHE: "my-cache"
"#
    .to_string()
}

fn generate_circleci_script() -> String {
    r#"version: 2.1

jobs:
  build:
    docker:
      - image: nixos/nix:latest
    steps:
      - checkout
      - run: nix build
      - run: |
          export FLAKECACHE_TOKEN="$FLAKECACHE_TOKEN"
          export FLAKECACHE_CACHE="${FLAKECACHE_CACHE:-my-cache}"
          bash scripts/flakecache-upload.sh

workflows:
  version: 2
  build-and-publish:
    jobs:
      - build:
          filters:
            branches:
              only: main
"#
    .to_string()
}

fn generate_travis_script() -> String {
    r#"language: nix

script:
  - nix build

after_success:
  - |
    export FLAKECACHE_TOKEN="$FLAKECACHE_TOKEN"
    export FLAKECACHE_CACHE="${FLAKECACHE_CACHE:-my-cache}"
    bash scripts/flakecache-upload.sh

branches:
  only:
    - main
"#
    .to_string()
}

fn generate_bitbucket_script() -> String {
    r#"image: nixos/nix:latest

pipelines:
  branches:
    main:
      - step:
          name: Build and Publish
          script:
            - nix build
            - |
              export FLAKECACHE_TOKEN="$FLAKECACHE_TOKEN"
              export FLAKECACHE_CACHE="${FLAKECACHE_CACHE:-my-cache}"
              bash scripts/flakecache-upload.sh
"#
    .to_string()
}

fn generate_buildkite_script() -> String {
    r#"steps:
  - label: "Build"
    commands:
      - nix build
    agents:
      queue: default

  - label: "Publish to FlakeCache"
    commands:
      - |
        export FLAKECACHE_TOKEN="$FLAKECACHE_TOKEN"
        export FLAKECACHE_CACHE="${FLAKECACHE_CACHE:-my-cache}"
        bash scripts/flakecache-upload.sh
    agents:
      queue: default
    branches: main
"#
    .to_string()
}

fn generate_tekton_script() -> String {
    r"apiVersion: tekton.dev/v1beta1
kind: Pipeline
metadata:
  name: flakecache-pipeline
spec:
  params:
    - name: cache-name
      description: FlakeCache cache name
      default: my-cache
  tasks:
    - name: build
      taskRef:
        name: nix-build
      params:
        - name: script
          value: |
            nix build

    - name: publish
      taskRef:
        name: flakecache-upload
      runAfter:
        - build
      params:
        - name: cache-name
          value: $(params.cache-name)
      env:
        - name: FLAKECACHE_TOKEN
          valueFrom:
            secretKeyRef:
              name: flakecache-secret
              key: token
        - name: FLAKECACHE_CACHE
          value: $(params.cache-name)
"
    .to_string()
}

fn generate_drone_script() -> String {
    r"kind: pipeline
type: docker
name: default

steps:
  - name: build
    image: nixos/nix:latest
    commands:
      - nix build

  - name: publish
    image: nixos/nix:latest
    environment:
      FLAKECACHE_TOKEN:
        from_secret: flakecache_token
      FLAKECACHE_CACHE: my-cache
    commands:
      - bash scripts/flakecache-upload.sh
    when:
      branch:
        - main
"
    .to_string()
}

fn generate_azure_devops_script() -> String {
    r#"trigger:
  branches:
    include:
      - main

pool:
  vmImage: 'ubuntu-latest'

steps:
  - task: UseNode@1
    displayName: 'Setup Nix'
    inputs:
      version: '18.x'

  - script: |
      curl -L https://nixos.org/nix/install | sh
      . $HOME/.nix-profile/etc/profile.d/nix.sh
      nix build
    displayName: 'Build'

  - script: |
      . $HOME/.nix-profile/etc/profile.d/nix.sh
      export FLAKECACHE_TOKEN="$(FLAKECACHE_TOKEN)"
      export FLAKECACHE_CACHE="$(FLAKECACHE_CACHE)"
      bash scripts/flakecache-upload.sh
    displayName: 'Publish to FlakeCache'
    env:
      FLAKECACHE_TOKEN: $(FLAKECACHE_TOKEN)
      FLAKECACHE_CACHE: $(FLAKECACHE_CACHE)
"#
    .to_string()
}

fn generate_aws_codebuild_script() -> String {
    r#"version: 0.2

phases:
  install:
    commands:
      - curl -L https://nixos.org/nix/install | sh
      - . $HOME/.nix-profile/etc/profile.d/nix.sh

  build:
    commands:
      - nix build

  post_build:
    commands:
      - |
        export FLAKECACHE_TOKEN="$FLAKECACHE_TOKEN"
        export FLAKECACHE_CACHE="${FLAKECACHE_CACHE:-my-cache}"
        bash scripts/flakecache-upload.sh

env:
  variables:
    FLAKECACHE_CACHE: my-cache
  secrets-manager:
    FLAKECACHE_TOKEN: flakecache/token:token
"#
    .to_string()
}

fn generate_gcp_cloudbuild_script() -> String {
    r#"steps:
  - name: 'nixos/nix:latest'
    entrypoint: 'nix'
    args: ['build']

  - name: 'nixos/nix:latest'
    entrypoint: 'bash'
    args:
      - '-c'
      - |
        export FLAKECACHE_TOKEN="$$FLAKECACHE_TOKEN"
        export FLAKECACHE_CACHE="$${FLAKECACHE_CACHE:-my-cache}"
        bash scripts/flakecache-upload.sh
    secretEnv: ['FLAKECACHE_TOKEN']

substitutions:
  _CACHE_NAME: 'my-cache'

options:
  machineType: 'N1_HIGHCPU_8'

availableSecrets:
  secretManager:
    - versionName: projects/$PROJECT_ID/secrets/flakecache-token/versions/latest
      env: 'FLAKECACHE_TOKEN'
"#
    .to_string()
}

fn generate_argocd_script() -> String {
    r"# ArgoCD Application for FlakeCache
# Note: ArgoCD is primarily a CD (Continuous Deployment) tool, not CI
# For building Nix packages, use a CI system (Jenkins, GitLab CI, etc.) to build and publish to FlakeCache
# Then ArgoCD can deploy the built artifacts to Kubernetes

apiVersion: argoproj.io/v1alpha1
kind: Application
metadata:
  name: flakecache-app
  namespace: argocd
spec:
  project: default
  source:
    repoURL: https://github.com/your-org/your-repo
    targetRevision: main
    path: k8s
  destination:
    server: https://kubernetes.default.svc
    namespace: default
  syncPolicy:
    automated:
      prune: true
      selfHeal: true
    syncOptions:
      - CreateNamespace=true

---
# Example: Use FlakeCache-built artifacts in Kubernetes
# The CI system builds and publishes to FlakeCache, then ArgoCD deploys
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-app
spec:
  replicas: 3
  template:
    spec:
      containers:
      - name: app
        image: your-registry/app:latest
        # Built via CI and published to FlakeCache
".to_string()
}

fn generate_teamcity_script() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<project>
  <build-types>
    <build-type id="FlakeCacheBuild">
      <name>Build and Publish to FlakeCache</name>
      <steps>
        <step name="Build" type="simpleRunner">
          <exec>
            <command>nix</command>
            <arguments>build</arguments>
          </exec>
        </step>
        <step name="Publish to FlakeCache" type="simpleRunner">
          <exec>
            <command>bash</command>
            <arguments>scripts/flakecache-upload.sh</arguments>
          </exec>
          <env>
            <envVar name="FLAKECACHE_TOKEN" value="%env.FLAKECACHE_TOKEN%"/>
            <envVar name="FLAKECACHE_CACHE" value="%env.FLAKECACHE_CACHE%"/>
          </env>
        </step>
      </steps>
      <vcs>
        <vcs-root-entries>
          <vcs-root-entry id="1"/>
        </vcs-root-entries>
      </vcs>
    </build-type>
  </build-types>
</project>
"#
    .to_string()
}

fn generate_bamboo_script() -> String {
    r#"version: 2
plan:
  project-key: FLAKECACHE
  key: BUILD
  name: Build and Publish

stages:
  - Build Stage:
      jobs:
        - Build Job:
            tasks:
              - script:
                  interpreter: SHELL
                  scripts:
                    - nix build

  - Publish Stage:
      jobs:
        - Publish Job:
            tasks:
              - script:
                  interpreter: SHELL
                  scripts:
                    - |
                      export FLAKECACHE_TOKEN="${bamboo.FLAKECACHE_TOKEN}"
                      export FLAKECACHE_CACHE="${bamboo.FLAKECACHE_CACHE:-my-cache}"
                      bash scripts/flakecache-upload.sh
            requires:
              - Build Job

variables:
  FLAKECACHE_CACHE: my-cache
"#
    .to_string()
}

fn generate_concourse_script() -> String {
    r#"---
resources:
  - name: source-code
    type: git
    source:
      uri: https://github.com/your-org/your-repo
      branch: main

jobs:
  - name: build-and-publish
    plan:
      - get: source-code
        trigger: true
      - task: build
        config:
          platform: linux
          image_resource:
            type: docker-image
            source:
              repository: nixos/nix
          run:
            path: nix
            args: [build]
      - task: publish
        config:
          platform: linux
          image_resource:
            type: docker-image
            source:
              repository: nixos/nix
          params:
            FLAKECACHE_TOKEN: ((flakecache-token))
            FLAKECACHE_CACHE: my-cache
          run:
            path: bash
            args:
              - -c
              - |
                export FLAKECACHE_TOKEN="${FLAKECACHE_TOKEN}"
                export FLAKECACHE_CACHE="${FLAKECACHE_CACHE}"
                bash scripts/flakecache-upload.sh
"#
    .to_string()
}

fn generate_spinnaker_script() -> String {
    r#"{
  "schema": "v2",
  "application": "flakecache-app",
  "name": "Build and Publish Pipeline",
  "stages": [
    {
      "refId": "1",
      "type": "jenkins",
      "name": "Build",
      "master": "jenkins",
      "job": "build-job",
      "parameters": {}
    },
    {
      "refId": "2",
      "type": "script",
      "name": "Publish to FlakeCache",
      "command": "bash scripts/flakecache-upload.sh",
      "image": "nixos/nix:latest",
      "env": {
        "FLAKECACHE_TOKEN": "${parameters.FLAKECACHE_TOKEN}",
        "FLAKECACHE_CACHE": "${parameters.FLAKECACHE_CACHE}"
      },
      "requisiteStageRefIds": ["1"]
    }
  ],
  "triggers": [
    {
      "type": "git",
      "branch": "main"
    }
  ],
  "parameters": [
    {
      "name": "FLAKECACHE_TOKEN",
      "description": "FlakeCache API token"
    },
    {
      "name": "FLAKECACHE_CACHE",
      "default": "my-cache",
      "description": "Cache name"
    }
  ]
}
"#
    .to_string()
}

fn generate_generic_script() -> String {
    r#"#!/bin/bash
# Generic FlakeCache upload script (bash)
# Works in any CI/CD system or locally
# This is the standalone script that all CI systems use

set -euo pipefail

# Configuration
FLAKECACHE_TOKEN="${FLAKECACHE_TOKEN:?FLAKECACHE_TOKEN environment variable is required}"
FLAKECACHE_CACHE="${FLAKECACHE_CACHE:?FLAKECACHE_CACHE environment variable is required}"
FLAKECACHE_API_URL="${FLAKECACHE_API_URL:-https://api.flakecache.com}"

echo "Publishing to FlakeCache: ${FLAKECACHE_CACHE}"

# Build Nix outputs
nix build --json

# Upload using the standalone script
bash scripts/flakecache-upload.sh
"#
    .to_string()
}
