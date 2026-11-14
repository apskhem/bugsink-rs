import type { ReactNode } from "react";
import { Link } from "react-router";

import { CopyButtonOnly } from "common/components/CopyButton";
import { routes } from "common/constants/routes";

import CodeBlock from "./CodeBlock";
import SdkSetupEventStatus from "./SdkSetupEventStatus";

export type SdkProps = { projectId: number; dsn: string; initialEventCount: number };

const link = "text-cyan-500 dark:text-cyan-300 font-bold";

/** The platforms upstream has instructions for, with their page's title (and heading). */
export const PLATFORMS = {
  "": "Connect your application",
  "python": "Connect your Python Application",
  "javascript": "Set up your SDK",
  "php": "Set up your SDK",
  "go": "Connect your Go Application"
} as const;
export type Platform = keyof typeof PLATFORMS;

export function isPlatform(platform: string): platform is Platform {
  return Object.hasOwn(PLATFORMS, platform);
}

function Page({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-center">
      <div className="m-4 max-w-4xl flex-auto">
        <h1 className="text-4xl font-bold">{title}</h1>
        {children}
      </div>
    </div>
  );
}

function OpenIssuesLink({ projectId }: { projectId: number }) {
  return (
    <Link to={routes.issues(projectId)} className={link}>
      list of open issues
    </Link>
  );
}

function SdkRecommendations() {
  return (
    <a href="https://www.bugsink.com/docs/sdk-recommendations/" className={link}>
      Bugsink-specific SDK recommendations
    </a>
  );
}

function Language({ projectId, platform, name }: { projectId: number; platform: Platform; name: string }) {
  return (
    <li>
      <Link
        className="text-cyan-500 dark:text-cyan-300 font-bold text-center"
        to={routes.projectSdkSetup(projectId, platform)}
      >
        <img src={`/images/lang-${platform}.png`} className="w-24 mx-4" alt={name} />
        <div className="mt-2">{name}</div>
      </Link>
    </li>
  );
}

/** `project_sdk_setup.html` */
export function GenericSetup({ projectId, dsn, initialEventCount }: SdkProps) {
  return (
    <Page title={PLATFORMS[""]}>
      <SdkSetupEventStatus projectId={projectId} initialEventCount={initialEventCount} />

      <div className="mt-4">
        Connect your application to Bugsink to start tracking errors. Bugsink is compatible with the{" "}
        <a href="https://docs.sentry.io/platforms/">Sentry SDKs</a>. Use the following DSN to connect your application
        to Bugsink:
      </div>
      <div className="flex items-start gap-2">
        <div className="flex-1 min-w-0">
          <CodeBlock language="text">{dsn}</CodeBlock>
        </div>
        <div className="mt-4">
          <CopyButtonOnly value={dsn} secondary />
        </div>
      </div>

      <h2 className="mt-8 text-2xl font-bold">Language-specific instructions</h2>
      <div className="mt-4">
        Pick your language to get specific instructions on how to set up the SDK:
        <ul className="flex mt-8">
          <Language projectId={projectId} platform="python" name="Python" />
          <Language projectId={projectId} platform="javascript" name="JavaScript" />
          <Language projectId={projectId} platform="php" name="PHP" />
          <Language projectId={projectId} platform="go" name="Go" />
        </ul>
      </div>
      <div className="mt-4">
        Not listed? It will probably work anyway! Just use the DSN above, and let us know if you run into any issues (on{" "}
        <a href="https://github.com/bugsink/bugsink/issues" className={link}>
          GitHub
        </a>{" "}
        or{" "}
        <a href="https://discord.gg/6Af6Yzz77C" className={link}>
          Discord
        </a>
        ).
      </div>

      <div className="mt-4">
        After triggering an event on purpose, it should now appear in the <OpenIssuesLink projectId={projectId} />.
      </div>
      <h2 className="mt-6 text-2xl font-bold">Further reading</h2>

      <div className="mt-4">
        For more information on how to use the SDK, check the <SdkRecommendations />.
      </div>
    </Page>
  );
}

/** `project_sdk_setup_python.html` */
export function PythonSetup({ projectId, dsn, initialEventCount }: SdkProps) {
  return (
    <Page title={PLATFORMS.python}>
      <SdkSetupEventStatus projectId={projectId} initialEventCount={initialEventCount} />

      <div className="mt-4">
        Connect your Python application to Bugsink to start tracking errors. Bugsink is compatible with the Sentry SDK.
        A basic setup is the following:
      </div>

      <h2 className="mt-8 text-2xl font-bold">Step 1: Install the SDK</h2>

      <div className="mt-4">Install the SDK using pip:</div>

      <CodeBlock language="text">pip install sentry-sdk</CodeBlock>

      <h2 className="mt-8 text-2xl font-bold">Step 2: Initialize the SDK</h2>

      <div className="mt-4">
        Initialize and configure the SDK with your DSN. Add the following at the start of your application code:
      </div>

      <CodeBlock language="python">{`import sentry_sdk

sentry_sdk.init(
    "${dsn}",

    send_default_pii=True,
    max_request_body_size="always",

    # Setting up the release is highly recommended. The SDK will try to
    # infer it, but explicitly setting it is more reliable:
    # release=...,

    # Don't event types which are not supported by Bugsink:
    traces_sample_rate=0,
    send_client_reports=False,
    auto_session_tracking=False,
)`}</CodeBlock>

      <h2 className="mt-6 text-2xl font-bold">Step 3: Verify the setup</h2>

      <div className="mt-4">
        To verify that everything is working, raise an exception on purpose and check that it appears in Bugsink.
      </div>
      <div className="mt-4">
        Put this code somewhere in your application where it can easily be triggered and then trigger it (don't use an
        interactive shell since the SDK will ignore exceptions raised in it):
      </div>

      <CodeBlock language="python">{`raise Exception("Raised Exception on purpose to send it to Bugsink")`}</CodeBlock>

      <div className="mt-4">
        Your event should now appear in the <OpenIssuesLink projectId={projectId} />.
      </div>

      <h2 className="mt-6 text-2xl font-bold">Further reading</h2>

      <div className="mt-4">
        For more information on how to use the SDK, check the <SdkRecommendations />.
      </div>
    </Page>
  );
}

/** `project_sdk_setup_javascript.html` */
export function JavaScriptSetup({ projectId, dsn, initialEventCount }: SdkProps) {
  return (
    <Page title={PLATFORMS.javascript}>
      <SdkSetupEventStatus projectId={projectId} initialEventCount={initialEventCount} />

      <div className="mt-4">
        Connect your JavaScript application to Bugsink to start tracking errors. Bugsink is compatible with the{" "}
        <a className={link} href="https://docs.sentry.io/platforms/javascript/">
          Sentry SDK
        </a>
        . Detailed instructions per framework are in the{" "}
        <a className={link} href="https://docs.sentry.io/platforms/javascript/">
          Sentry SDK Documentation
        </a>
        . In the below we provide an overview, zoom in on the differences between Bugsink and Sentry, and provide a
        snippet with the correct DSN set.
      </div>

      <h2 className="mt-8 text-2xl font-bold">Step 1: Install the SDK</h2>

      <div className="mt-4">
        Install the SDK by including a script tag, or by using a package manager such as npm or yarn. Note that the SDK
        to use depends on your framework (e.g. React, Angular, Vue, etc.).
        <CodeBlock language="text">npm install @sentry/your-js-flavor --save</CodeBlock>
      </div>

      <h2 className="mt-8 text-2xl font-bold">Step 2: Initialize the SDK</h2>

      <div className="mt-4">
        Initialize and configure the SDK with your DSN. Use the configuration snippet that applies to your framework,
        and replace the Sentry.init call with the call below:
      </div>

      <CodeBlock language="javascript">{`// use the correct import statement for your framework, like:

// import * as Sentry from "@sentry/browser";
// const Sentry = require("@sentry/node");
// import * as Sentry from "@sentry/angular";

Sentry.init({
  dsn: "${dsn}",

  // Alternatively, use \`process.env.npm_package_version\` for a dynamic release version
  // if your build tool supports it.
  release: "my-project-name@...",

  integrations: [],
  tracesSampleRate: 0,
});`}</CodeBlock>

      <h2 className="mt-6 text-2xl font-bold">Step 3: Verify the setup</h2>

      <div className="mt-4">
        To verify that everything is working, raise an exception on purpose and check that it appears in Bugsink. A good
        location for this depends on your framework, but a good place is in the main entry point of your application.
        (Don't use the devtools console, because it will not be caught by the SDK).
      </div>

      <CodeBlock language="javascript">{`// Put something like this in the handler of a button click, on a timeout, or similar.
throw new Error("Error Thrown on purpose to send it to Bugsink");`}</CodeBlock>

      <div className="mt-4">
        Your event should now appear in the <OpenIssuesLink projectId={projectId} />.
      </div>

      <h2 className="mt-6 text-2xl font-bold">Further reading</h2>

      <div className="mt-4">
        You'll probably want to configure the{" "}
        <a href="https://www.bugsink.com/docs/sourcemaps/" className={link}>
          source maps
        </a>{" "}
        next, so that you get readable stack traces. For even more information on how to use the SDK, check the{" "}
        <SdkRecommendations />.
      </div>
    </Page>
  );
}

/** `project_sdk_setup_php.html` */
export function PhpSetup({ projectId, dsn, initialEventCount }: SdkProps) {
  return (
    <Page title={PLATFORMS.php}>
      <SdkSetupEventStatus projectId={projectId} initialEventCount={initialEventCount} />

      <div className="mt-4">
        Connect your PHP application to Bugsink to start tracking errors. Bugsink is compatible with the{" "}
        <a className={link} href="https://docs.sentry.io/platforms/php/">
          Sentry SDK
        </a>
        . Note that the instructions for{" "}
        <a className={link} href="https://docs.sentry.io/platforms/php/guides/laravel/">
          Laravel
        </a>{" "}
        and{" "}
        <a className={link} href="https://docs.sentry.io/platforms/php/guides/symfony/">
          Symfony
        </a>{" "}
        are quite different from plain PHP. In the below we provide an overview, zoom in on the differences between
        Bugsink and Sentry, and provide a snippet with the correct DSN set.
      </div>

      <h2 className="mt-8 text-2xl font-bold">Step 1: Install the SDK</h2>

      <div className="mt-4">
        Install the SDK using Composer. Run either of the following commands:
        <CodeBlock language="text">{`PICK ONE:
composer require sentry/sentry
composer require sentry/sentry-laravel
composer require sentry/sentry-symfony`}</CodeBlock>
      </div>

      <h2 className="mt-8 text-2xl font-bold">Step 2: Configure the SDK with your DSN</h2>

      <div className="mt-4">
        The steps for initialization/configuration differ between the different frameworks. Refer to the relevant
        document.
      </div>
      <div className="mt-4">
        Configure the SDK without tracing and profiling, as Bugsink does not support these features.
      </div>
      <div className="mt-4">Initialize and configure the SDK with your DSN:</div>

      {/* Pygments' PHP lexer leaves text outside `<?php` unhighlighted */}
      <CodeBlock language="text">{dsn}</CodeBlock>

      <h2 className="mt-6 text-2xl font-bold">Step 3: Verify the setup</h2>

      <div className="mt-4">
        To verify that everything is working, raise an exception on purpose and check that it appears in Bugsink. How to
        do this depends on the framework you are using, as per the documentation. Here's an example for plain PHP:
      </div>

      <CodeBlock language="php">{`<?php
try {
  $this->iWantThisToFailSoBugsinkShowsIt();
} catch (\\Throwable $exception) {
  \\Sentry\\captureException($exception);
}
?>`}</CodeBlock>

      <div className="mt-4">
        Your event should now appear in the <OpenIssuesLink projectId={projectId} />.
      </div>

      <h2 className="mt-6 text-2xl font-bold">Further reading</h2>

      <div className="mt-4">
        For more information on how to use the SDK, check the <SdkRecommendations />.
      </div>
    </Page>
  );
}

/** `project_sdk_setup_go.html` */
export function GoSetup({ projectId, dsn, initialEventCount }: SdkProps) {
  return (
    <Page title={PLATFORMS.go}>
      <SdkSetupEventStatus projectId={projectId} initialEventCount={initialEventCount} />

      <div className="mt-4">
        Connect your Go application to Bugsink to start tracking errors. Bugsink is compatible with the{" "}
        <a className={link} href="https://docs.sentry.io/platforms/go/">
          Sentry SDK
        </a>
        . A basic setup is the following:
      </div>

      <h2 className="mt-8 text-2xl font-bold">Step 1: Install the SDK</h2>

      <div className="mt-4">
        Install the SDK using <code>go get</code>:
      </div>

      <CodeBlock language="text">go get github.com/getsentry/sentry-go@latest</CodeBlock>

      <h2 className="mt-8 text-2xl font-bold">Step 2: Initialize the SDK</h2>

      <div className="mt-4">Initialize and configure the SDK with your DSN:</div>

      <CodeBlock language="go">{`package main

import (
    "log"
    "time"

    "github.com/getsentry/sentry-go"
)

func main() {
    err := sentry.Init(sentry.ClientOptions{
        Dsn: "${dsn}",

        SendDefaultPII: true,

        EnableTracing: false,
        TracesSampleRate: 0,
        DisableLogs: true,
        DisableMetrics: true,
        DisableClientReports: true,
    })
    if err != nil {
        log.Fatalf("sentry.Init: %s", err)
    }
    defer sentry.Flush(2 * time.Second)

    // Your application code starts here.
}`}</CodeBlock>

      <h2 className="mt-6 text-2xl font-bold">Step 3: Verify the setup</h2>

      <div className="mt-4">
        To verify that everything is working, create an error on purpose and check that it appears in Bugsink.
      </div>
      <div className="mt-4">
        Put this code in your application where it can easily be triggered and then trigger it:
      </div>

      <CodeBlock language="go">{`sentry.CaptureMessage("Error created on purpose to send it to Bugsink")`}</CodeBlock>

      <div className="mt-4">
        Your event should now appear in the <OpenIssuesLink projectId={projectId} />.
      </div>

      <h2 className="mt-6 text-2xl font-bold">Further reading</h2>

      <div className="mt-4">
        For more information on how to use the SDK, check the <SdkRecommendations />.
      </div>
    </Page>
  );
}
